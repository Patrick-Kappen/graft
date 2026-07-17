use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context as _, Result};
use clap::{Parser, ValueEnum};
use graft::manifest::{ManifestLoader, ProducerIdentity};
#[cfg(feature = "worker-test-fixtures")]
use graft::protocol::Capability;
use graft::protocol::{
    CapabilitySet, EffectiveLimits, ManagerKind, ManifestGeneration, ManifestState,
    ManifestUnavailableReason, ProtocolVersionRange, SoftwareVersion, WorkerContext, WorkerTarget,
    PROTOCOL_MAJOR, PROTOCOL_MAX_MINOR, PROTOCOL_MIN_MINOR,
};
use graft::worker::activation;
#[cfg(feature = "worker-test-fixtures")]
use graft::worker::MockDispatcher;
#[cfg(not(feature = "worker-test-fixtures"))]
use graft::worker::UnsupportedDispatcher;
use graft::worker::{serve, ServerConfig};

#[derive(Debug, Parser)]
struct Arguments {
    #[arg(long, value_enum)]
    target: TargetArgument,
    #[arg(long)]
    effective_uid: u32,
    #[arg(long, value_enum)]
    manager: ManagerArgument,
    #[arg(long)]
    config_home: Option<PathBuf>,
    #[arg(long)]
    graft_gid: Option<u32>,
    #[arg(long)]
    producer_name: String,
    #[arg(long)]
    producer_version: String,
    #[arg(long)]
    producer_build_id: String,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum TargetArgument {
    System,
    User,
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ManagerArgument {
    System,
    User,
}

#[tokio::main]
async fn main() -> Result<()> {
    let arguments = Arguments::parse();
    let listener = activation::take_listener().context("socket activation validation failed")?;
    let target = match arguments.target {
        TargetArgument::System => WorkerTarget::System,
        TargetArgument::User => WorkerTarget::User,
    };
    let manager = match arguments.manager {
        ManagerArgument::System => ManagerKind::System,
        ManagerArgument::User => ManagerKind::User,
    };
    if arguments.effective_uid != rustix::process::geteuid().as_raw() {
        bail!("configured effective UID does not match the worker process");
    }
    let context = WorkerContext::new(target, arguments.effective_uid, manager)
        .context("invalid fixed worker context")?;
    let producer = ProducerIdentity::new(
        &arguments.producer_name,
        &arguments.producer_version,
        &arguments.producer_build_id,
    )
    .context("invalid installed producer identity")?;
    let loader = match target {
        WorkerTarget::System => {
            if arguments.config_home.is_some() {
                bail!("system worker does not accept --config-home");
            }
            ManifestLoader::system(
                arguments
                    .graft_gid
                    .context("system worker requires --graft-gid")?,
                producer,
            )
        }
        WorkerTarget::User => {
            if arguments.graft_gid.is_some() {
                bail!("user worker does not accept --graft-gid");
            }
            ManifestLoader::user(
                &arguments
                    .config_home
                    .context("user worker requires --config-home")?,
                arguments.effective_uid,
                rustix::process::getgid().as_raw(),
                producer,
            )?
        }
    };
    let manifest = match loader.load() {
        Ok(snapshot) => ManifestState::Available {
            generation: ManifestGeneration::parse(snapshot.manifest().generation_id().as_str())?,
        },
        Err(_) => ManifestState::Unavailable {
            reason: ManifestUnavailableReason::Invalid,
        },
    };
    #[cfg(feature = "worker-test-fixtures")]
    let (capabilities, dispatcher) = (
        CapabilitySet::new([Capability::Observe])?,
        Arc::new(MockDispatcher) as Arc<dyn graft::worker::SemanticDispatcher>,
    );
    #[cfg(not(feature = "worker-test-fixtures"))]
    let (capabilities, dispatcher) = (
        CapabilitySet::new([])?,
        Arc::new(UnsupportedDispatcher) as Arc<dyn graft::worker::SemanticDispatcher>,
    );
    let config = ServerConfig {
        context,
        protocol: ProtocolVersionRange::new(
            PROTOCOL_MAJOR,
            PROTOCOL_MIN_MINOR,
            PROTOCOL_MAX_MINOR,
        )?,
        software_version: SoftwareVersion::parse(env!("CARGO_PKG_VERSION"))?,
        capabilities,
        limits: EffectiveLimits::protocol_maxima(),
        manifest,
        dispatcher,
    };
    serve(listener, config).await?;
    Ok(())
}
