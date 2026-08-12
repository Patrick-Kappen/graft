#![deny(clippy::all)]
#![deny(clippy::pedantic)]

//! Internal NixOS activation helper for one immutable discovery generation.

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use clap::Parser;
use graft::manifest::{publish_system_generation, ProducerIdentity};

#[derive(Debug, Parser)]
#[command(name = "graft-manifest-publish-system")]
struct Arguments {
    /// Direct immutable Nix store generation to publish.
    generation: PathBuf,
    /// Runtime GID of the fixed NixOS graft readership group.
    #[arg(long)]
    graft_gid: u32,
    /// Installed producer package name.
    #[arg(long)]
    producer_name: String,
    /// Installed producer semantic version.
    #[arg(long)]
    producer_version: String,
    /// Installed producer source/build identity.
    #[arg(long)]
    producer_build_id: String,
}

fn main() -> Result<()> {
    let arguments = Arguments::parse();
    let producer = ProducerIdentity::new(
        &arguments.producer_name,
        &arguments.producer_version,
        &arguments.producer_build_id,
    )
    .context("invalid installed producer identity")?;
    publish_system_generation(&arguments.generation, arguments.graft_gid, producer)
        .context("cannot publish system manifest generation")
}
