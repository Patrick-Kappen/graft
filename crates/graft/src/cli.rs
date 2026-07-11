//! CLI argument parsing and command dispatch.

use std::collections::BTreeSet;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;

use crate::config::ContainerConfig;
use crate::resolve::{self, ConfigSource, ResolvedContainer};

#[derive(Parser)]
#[command(name = "graft", about = "TOML → resolved JSON generator")]
struct Cli {
    /// TOML container config to resolve.
    toml_file: PathBuf,
    /// Explicit TOML source available for cross-workload references.
    #[arg(long = "context", value_name = "TOML")]
    context_files: Vec<PathBuf>,
}

struct LoadedSource {
    unit_name: String,
    config: ContainerConfig,
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
    let resolved = resolve_cli(&cli)?;

    let stdout = std::io::stdout();
    let mut stdout = stdout.lock();
    serde_json::to_writer_pretty(&mut stdout, &resolved)
        .context("failed to write resolved JSON")?;
    writeln!(stdout).context("failed to write trailing newline")?;

    Ok(())
}

fn resolve_cli(cli: &Cli) -> Result<ResolvedContainer> {
    let mut paths = Vec::with_capacity(cli.context_files.len() + 1);
    paths.push(cli.toml_file.clone());
    paths.extend(cli.context_files.iter().cloned());

    let mut seen = BTreeSet::new();
    paths.retain(|path| seen.insert(path.clone()));

    let loaded = paths
        .iter()
        .map(|path| load_source(path))
        .collect::<Result<Vec<_>>>()?;
    let sources = loaded
        .iter()
        .map(|source| ConfigSource::new(&source.unit_name, &source.config))
        .collect::<Vec<_>>();

    resolve::resolve_with_context(&loaded[0].config, &sources)
}

fn load_source(path: &Path) -> Result<LoadedSource> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read config: {}", path.display()))?;
    let config = toml::from_str(&content)
        .with_context(|| format!("failed to parse config: {}", path.display()))?;
    let unit_name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .with_context(|| format!("config path has no UTF-8 filename stem: {}", path.display()))?
        .to_string();

    Ok(LoadedSource { unit_name, config })
}

#[cfg(test)]
mod tests {
    use std::fs;

    use clap::Parser as _;
    use tempfile::tempdir;

    use super::*;
    use crate::resolve::ResolvedNetworkNamespace;

    #[test]
    fn parses_repeated_context_arguments() {
        let cli = Cli::try_parse_from([
            "graft",
            "worker.toml",
            "--context",
            "database.toml",
            "--context",
            "cache.toml",
        ])
        .unwrap();

        assert_eq!(cli.toml_file, PathBuf::from("worker.toml"));
        assert_eq!(
            cli.context_files,
            [PathBuf::from("database.toml"), PathBuf::from("cache.toml")]
        );
    }

    #[test]
    fn resolves_container_reference_from_explicit_context() {
        let directory = tempdir().unwrap();
        let worker = directory.path().join("worker.toml");
        let database = directory.path().join("database-source.toml");
        fs::write(
            &worker,
            r#"
                version = 1
                name = "worker"

                [config.network]
                mode = "container"
                container = "database"
            "#,
        )
        .unwrap();
        fs::write(&database, "version = 1\nname = \"database\"\n").unwrap();
        let cli = Cli {
            toml_file: worker,
            context_files: vec![database.clone(), database],
        };

        let resolved = resolve_cli(&cli).unwrap();
        let namespace = resolved.network.unwrap().namespace.unwrap();

        assert_eq!(
            namespace,
            ResolvedNetworkNamespace::Container {
                unit: "database-source.container".to_string()
            }
        );
    }

    #[test]
    fn reports_context_parse_path() {
        let directory = tempdir().unwrap();
        let worker = directory.path().join("worker.toml");
        let invalid = directory.path().join("invalid.toml");
        fs::write(&worker, "version = 1\nname = \"worker\"\n").unwrap();
        fs::write(&invalid, "[[[").unwrap();
        let cli = Cli {
            toml_file: worker,
            context_files: vec![invalid.clone()],
        };

        let error = resolve_cli(&cli).unwrap_err();

        assert!(error.to_string().contains(&invalid.display().to_string()));
    }
}
