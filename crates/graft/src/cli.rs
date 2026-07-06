//! CLI argument parsing and command dispatch.

use std::io::Write as _;
use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::Parser;

use crate::config::ContainerConfig;
use crate::resolve;

#[derive(Parser)]
#[command(name = "graft", about = "TOML → resolved JSON generator")]
struct Cli {
    /// TOML container config to resolve.
    toml_file: PathBuf,
}

/// Parse CLI arguments and write the resolved JSON spec to stdout.
///
/// # Errors
///
/// Returns an error if the config cannot be read, parsed, resolved, or written.
pub fn run() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive(tracing::Level::WARN.into()),
        )
        .init();

    let cli = Cli::parse();
    let content = std::fs::read_to_string(&cli.toml_file)
        .with_context(|| format!("cannot read config: {}", cli.toml_file.display()))?;
    let config: ContainerConfig = toml::from_str(&content)
        .with_context(|| format!("failed to parse config: {}", cli.toml_file.display()))?;
    let resolved = resolve::resolve(&config)?;

    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    serde_json::to_writer_pretty(&mut stdout, &resolved)
        .context("failed to write resolved JSON")?;
    writeln!(stdout).context("failed to write trailing newline")?;

    Ok(())
}
