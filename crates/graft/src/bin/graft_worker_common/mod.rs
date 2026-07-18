use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{bail, Context as _, Result};
use clap::{Parser, ValueEnum};
use graft::manifest::{ManifestError, ManifestLoader, ProducerIdentity};
use graft::protocol::{
    CapabilitySet, EffectiveLimits, ManagerKind, ManifestGeneration, ManifestState,
    ManifestUnavailableReason, ProtocolVersionRange, SoftwareVersion, WorkerContext, WorkerTarget,
    PROTOCOL_MAJOR, PROTOCOL_MAX_MINOR, PROTOCOL_MIN_MINOR,
};
use graft::worker::activation;
use graft::worker::{serve, SemanticDispatcher, ServerConfig};

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

pub(crate) struct Prepared {
    arguments: Arguments,
    listener: std::os::unix::net::UnixListener,
}

pub(crate) fn prepare() -> Result<Prepared> {
    let arguments = Arguments::parse();
    let listener = activation::take_listener().context("socket activation validation failed")?;
    Ok(Prepared {
        arguments,
        listener,
    })
}

pub(crate) async fn run(
    prepared: Prepared,
    capabilities: CapabilitySet,
    dispatcher: Arc<dyn SemanticDispatcher>,
) -> Result<()> {
    let Prepared {
        arguments,
        listener,
    } = prepared;
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
        Err(error) => ManifestState::Unavailable {
            reason: manifest_unavailable_reason(&error),
        },
    };
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

fn manifest_unavailable_reason(error: &ManifestError) -> ManifestUnavailableReason {
    match error {
        ManifestError::MissingCurrent => ManifestUnavailableReason::Missing,
        ManifestError::Filesystem(_)
        | ManifestError::FileType
        | ManifestError::Ownership
        | ManifestError::Permissions
        | ManifestError::UnexpectedEntry
        | ManifestError::GenerationReference
        | ManifestError::DocumentTooLarge => ManifestUnavailableReason::Unreadable,
        ManifestError::IncompatibleSchema
        | ManifestError::ApiCompatibility
        | ManifestError::ProducerMismatch
        | ManifestError::InstalledProducerMismatch => ManifestUnavailableReason::Incompatible,
        _ => ManifestUnavailableReason::Invalid,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_unavailability_preserves_missing_incompatible_and_unreadable_classes() {
        assert_eq!(
            manifest_unavailable_reason(&ManifestError::MissingCurrent),
            ManifestUnavailableReason::Missing
        );
        assert_eq!(
            manifest_unavailable_reason(&ManifestError::Filesystem(std::io::Error::from(
                std::io::ErrorKind::NotFound
            ))),
            ManifestUnavailableReason::Unreadable
        );
        assert_eq!(
            manifest_unavailable_reason(&ManifestError::IncompatibleSchema),
            ManifestUnavailableReason::Incompatible
        );
        assert_eq!(
            manifest_unavailable_reason(&ManifestError::Permissions),
            ManifestUnavailableReason::Unreadable
        );
    }
}
