#![deny(clippy::all)]
#![deny(clippy::pedantic)]

//! Internal Home Manager activation helper for one immutable discovery generation.

use std::path::PathBuf;

use anyhow::{Context as _, Result};
use clap::Parser;
use graft::manifest::{publish_user_generation, ProducerIdentity};

#[derive(Debug, Parser)]
#[command(name = "graft-manifest-publish-user")]
struct Arguments {
    /// Direct immutable Nix store generation to publish.
    generation: PathBuf,
    /// Fixed Home Manager config home.
    #[arg(long)]
    config_home: PathBuf,
    /// Fixed Home Manager state home.
    #[arg(long)]
    state_home: PathBuf,
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
    publish_user_generation(
        &arguments.config_home,
        &arguments.state_home,
        &arguments.generation,
        producer,
    )
    .context("cannot publish user manifest generation")
}
