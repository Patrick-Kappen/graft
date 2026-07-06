//! CLI argument parsing and command dispatch.

use anyhow::Result;
use clap::Parser;

#[derive(Parser)]
#[command(name = "graft", about = "TOML → Quadlet config file generator")]
struct Cli {}

/// Parse CLI arguments and dispatch to the appropriate command.
///
/// # Errors
///
/// Returns an error if the command fails.
pub fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .init();

    let _cli = Cli::parse();

    Ok(())
}
