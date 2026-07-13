//! CLI argument parsing and command dispatch.

use std::collections::{BTreeMap, BTreeSet};
use std::io::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use clap::Parser;
use serde::Serialize;

use crate::config::ContainerConfig;
use crate::resolve::{self, ConfigSource, ResolvedContainer};

#[derive(Parser)]
#[command(name = "graft", about = "TOML → resolved JSON generator")]
struct Cli {
    /// TOML container config to resolve.
    #[arg(required_unless_present = "set_files")]
    toml_file: Option<PathBuf>,
    /// Explicit TOML source available for cross-workload references.
    #[arg(long = "context", value_name = "TOML", requires = "toml_file")]
    context_files: Vec<PathBuf>,
    /// Resolve a complete explicit TOML source set in one pass.
    #[arg(
        long = "set",
        value_name = "TOML",
        num_args = 1..,
        conflicts_with_all = ["toml_file", "context_files"]
    )]
    set_files: Vec<PathBuf>,
}

struct LoadedSource {
    file_name: String,
    unit_name: String,
    origin: String,
    config: ContainerConfig,
}

#[derive(Debug, Serialize)]
#[serde(untagged)]
enum CliOutput {
    Single(Box<ResolvedContainer>),
    Set(BTreeMap<String, ResolvedContainer>),
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

fn resolve_cli(cli: &Cli) -> Result<CliOutput> {
    if !cli.set_files.is_empty() {
        return resolve_set(&cli.set_files).map(CliOutput::Set);
    }

    let toml_file = cli
        .toml_file
        .as_ref()
        .context("TOML container config is required")?;
    let mut paths = Vec::with_capacity(cli.context_files.len() + 1);
    paths.push(toml_file.clone());
    paths.extend(cli.context_files.iter().cloned());

    let mut seen = BTreeSet::new();
    paths.retain(|path| seen.insert(path.clone()));

    let loaded = load_sources(&paths)?;
    let sources = config_sources(&loaded);
    resolve::resolve_with_context(&loaded[0].config, &sources)
        .map(Box::new)
        .map(CliOutput::Single)
}

fn resolve_set(paths: &[PathBuf]) -> Result<BTreeMap<String, ResolvedContainer>> {
    let loaded = load_sources(paths)?;
    let mut file_names = BTreeSet::new();
    for source in &loaded {
        if !file_names.insert(source.file_name.as_str()) {
            anyhow::bail!(
                "duplicate TOML filename in explicit set: {}",
                source.file_name
            );
        }
    }

    let sources = config_sources(&loaded);
    let resolved = resolve::resolve_set(&sources)?;
    Ok(loaded
        .iter()
        .zip(resolved)
        .map(|(source, container)| (source.file_name.clone(), container))
        .collect())
}

fn load_sources(paths: &[PathBuf]) -> Result<Vec<LoadedSource>> {
    paths.iter().map(|path| load_source(path)).collect()
}

fn config_sources(loaded: &[LoadedSource]) -> Vec<ConfigSource<'_>> {
    loaded
        .iter()
        .map(|source| ConfigSource::with_origin(&source.unit_name, &source.origin, &source.config))
        .collect()
}

fn load_source(path: &Path) -> Result<LoadedSource> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read config: {}", path.display()))?;
    let config = toml::from_str(&content)
        .with_context(|| format!("failed to parse config: {}", path.display()))?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .with_context(|| format!("config path has no UTF-8 filename: {}", path.display()))?
        .to_string();
    let unit_name = path
        .file_stem()
        .and_then(|stem| stem.to_str())
        .with_context(|| format!("config path has no UTF-8 filename stem: {}", path.display()))?
        .to_string();

    Ok(LoadedSource {
        file_name,
        unit_name,
        origin: path.display().to_string(),
        config,
    })
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

        assert_eq!(cli.toml_file, Some(PathBuf::from("worker.toml")));
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
            toml_file: Some(worker),
            context_files: vec![database.clone(), database],
            set_files: Vec::new(),
        };

        let CliOutput::Single(resolved) = resolve_cli(&cli).unwrap() else {
            panic!("single-file CLI should return one container");
        };
        let namespace = resolved.network.unwrap().namespace.unwrap();

        assert_eq!(
            namespace,
            ResolvedNetworkNamespace::Container {
                unit: "database-source.container".to_string()
            }
        );
    }

    #[test]
    fn resolves_workload_dependency_from_explicit_set() {
        let directory = tempdir().unwrap();
        let worker = directory.path().join("worker.toml");
        let database = directory.path().join("database-source.toml");
        fs::write(
            &worker,
            r#"
                version = 1
                name = "worker"

                [[dependencies]]
                target = { workload = "database" }
                requirement = "required"
                ordering = "after"
            "#,
        )
        .unwrap();
        fs::write(&database, "version = 1\nname = \"database\"\n").unwrap();
        let cli = Cli {
            toml_file: None,
            context_files: Vec::new(),
            set_files: vec![worker, database],
        };

        let set_output = |output| match output {
            CliOutput::Set(resolved) => Some(resolved),
            CliOutput::Single(_) => None,
        };
        let resolved = set_output(resolve_cli(&cli).unwrap())
            .expect("set CLI should return containers by TOML filename");
        let dependencies = resolved["worker.toml"].dependencies.as_ref().unwrap();

        assert_eq!(
            dependencies.requires,
            ["database-source.container".to_string()]
        );
        assert_eq!(
            dependencies.after,
            ["database-source.container".to_string()]
        );

        let single_output = CliOutput::Single(Box::new(resolved["worker.toml"].clone()));
        assert!(set_output(single_output).is_none());
    }

    #[test]
    fn reports_context_parse_path() {
        let directory = tempdir().unwrap();
        let worker = directory.path().join("worker.toml");
        let invalid = directory.path().join("invalid.toml");
        fs::write(&worker, "version = 1\nname = \"worker\"\n").unwrap();
        fs::write(&invalid, "[[[").unwrap();
        let cli = Cli {
            toml_file: Some(worker),
            context_files: vec![invalid.clone()],
            set_files: Vec::new(),
        };

        let error = resolve_cli(&cli).unwrap_err();

        assert!(error.to_string().contains(&invalid.display().to_string()));
    }

    #[test]
    fn resolves_explicit_set_once_with_path_context() {
        let directory = tempdir().unwrap();
        let worker = directory.path().join("worker.toml");
        let database = directory.path().join("database.toml");
        fs::write(
            &worker,
            "version = 1\nname = \"worker\"\n[config.network]\nmode = \"container\"\ncontainer = \"database\"\n",
        )
        .unwrap();
        fs::write(&database, "version = 1\nname = \"database\"\n").unwrap();
        let cli = Cli {
            toml_file: None,
            context_files: Vec::new(),
            set_files: vec![worker, database],
        };

        let CliOutput::Set(resolved) = resolve_cli(&cli).unwrap() else {
            panic!("set CLI should return containers by TOML filename");
        };

        assert_eq!(resolved.len(), 2);
        assert!(resolved.contains_key("worker.toml"));
        assert!(resolved.contains_key("database.toml"));
    }

    #[test]
    fn missing_set_reference_error_contains_source_path() {
        let directory = tempdir().unwrap();
        let worker = directory.path().join("worker.toml");
        fs::write(
            &worker,
            "version = 1\nname = \"worker\"\n[config.network]\nmode = \"container\"\ncontainer = \"missing\"\n",
        )
        .unwrap();
        let cli = Cli {
            toml_file: None,
            context_files: Vec::new(),
            set_files: vec![worker.clone()],
        };

        let error = resolve_cli(&cli).unwrap_err();

        assert!(error.to_string().contains(&worker.display().to_string()));
    }

    #[test]
    fn semantic_set_error_contains_source_path() {
        let directory = tempdir().unwrap();
        let worker = directory.path().join("worker.toml");
        let invalid = directory.path().join("invalid.toml");
        fs::write(
            &worker,
            "version = 1\nname = \"worker\"\n[config.network]\nmode = \"container\"\ncontainer = \"invalid\"\n",
        )
        .unwrap();
        fs::write(&invalid, "name = \"invalid\"\n").unwrap();
        let cli = Cli {
            toml_file: None,
            context_files: Vec::new(),
            set_files: vec![worker, invalid.clone()],
        };

        let error = resolve_cli(&cli).unwrap_err();

        assert!(error.to_string().contains(&invalid.display().to_string()));
    }

    #[test]
    fn unsupported_set_field_error_contains_field_and_source_path() {
        let directory = tempdir().unwrap();
        let unsupported = directory.path().join("unsupported.toml");
        fs::write(
            &unsupported,
            "version = 1\nname = \"unsupported\"\n[config.resources]\nmemory = \"512m\"\n",
        )
        .unwrap();
        let cli = Cli {
            toml_file: None,
            context_files: Vec::new(),
            set_files: vec![unsupported.clone()],
        };

        let error = resolve_cli(&cli).unwrap_err();
        let diagnostic = format!("{error:#}");

        assert!(diagnostic.contains(&unsupported.display().to_string()));
        assert!(diagnostic.contains("config.resources.memory is configured but not implemented"));
    }
}
