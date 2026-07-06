//! CLI argument parsing and command dispatch.

use anyhow::Result;
use clap::{Parser, Subcommand};

use crate::commands;
use crate::container::podman::Podman;

#[derive(Parser)]
#[command(name = "graft", about = "Manage Graft containers")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Start container if not running, then open an interactive shell.
    Shell {
        /// Container name (matches TOML filename without extension).
        name: String,
    },
}

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

    let cli = Cli::parse();
    let runtime = Podman::new();

    match cli.command {
        Command::Shell { name } => commands::shell::run(&runtime, &name),
    }
}
