//! TOML-file-based configuration provider.
//!
//! Reads `.graft/containers/<name>.toml` from a given directory.
//! The file name is always the container name — any `name` field in the TOML
//! is overwritten by the loader.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::config::{ConfigProvider, ContainerConfig};

/// Loads container configs from a `containers/` directory.
///
/// By default rooted at `.graft/containers/` in the current working directory.
pub struct TomlConfig {
    containers_dir: PathBuf,
}

impl TomlConfig {
    /// Creates a [`TomlConfig`] rooted at `containers_dir`.
    #[must_use]
    pub fn new(containers_dir: impl Into<PathBuf>) -> Self {
        Self {
            containers_dir: containers_dir.into(),
        }
    }

    /// Creates a [`TomlConfig`] from `.graft/containers/` in the current
    /// working directory.
    ///
    /// # Errors
    ///
    /// Returns an error if the current directory cannot be determined.
    pub fn from_cwd() -> Result<Self> {
        let cwd = std::env::current_dir().context("failed to determine current directory")?;
        Ok(Self::new(cwd.join(".graft/containers")))
    }
}

impl ConfigProvider for TomlConfig {
    fn load(&self, name: &str) -> Result<ContainerConfig> {
        let path = self.containers_dir.join(format!("{name}.toml"));
        load_from_path(&path, name)
    }
}

/// Reads and parses a TOML file at `path`.
///
/// The file name (`name`) always becomes the container name — any `name`
/// field inside the TOML is overwritten.
///
/// # Errors
///
/// Returns an error if the file cannot be read or the TOML is invalid.
fn load_from_path(path: &Path, name: &str) -> Result<ContainerConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("cannot read config: {}", path.display()))?;
    parse_toml(&content, name)
        .with_context(|| format!("failed to parse config: {}", path.display()))
}

/// Parses TOML `content` and sets `name` as the container name.
///
/// Exposed for testing without requiring a real filesystem.
///
/// # Errors
///
/// Returns an error if the TOML is malformed or contains unknown fields.
pub(crate) fn parse_toml(content: &str, name: &str) -> Result<ContainerConfig> {
    let mut config: ContainerConfig = toml::from_str(content).context("TOML parse error")?;
    // Filename is always leading — overwrite whatever the TOML says.
    config.name = Some(name.to_string());
    Ok(config)
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;
    use crate::config::schema::DeployTarget;

    // ── parse_toml ────────────────────────────────────────────────────────

    #[test]
    fn empty_toml_is_valid() {
        let cfg = parse_toml("", "mycontainer").unwrap();
        assert_eq!(cfg.name.as_deref(), Some("mycontainer"));
    }

    #[test]
    fn filename_overrides_name_in_toml() {
        let cfg = parse_toml(r#"name = "other""#, "mycontainer").unwrap();
        assert_eq!(cfg.name.as_deref(), Some("mycontainer"));
    }

    #[test]
    fn parses_version_and_deploy() {
        let toml = r#"
            version = 1
            [deploy]
            enable = true
            target = "user"
        "#;
        let cfg = parse_toml(toml, "srv").unwrap();
        assert_eq!(cfg.version, Some(1));
        let deploy = cfg.deploy.unwrap();
        assert_eq!(deploy.enable, Some(true));
        assert_eq!(deploy.target, Some(DeployTarget::User));
    }

    #[test]
    fn parses_runtime_section() {
        let toml = r#"
            [config.runtime]
            mode = "rootfs-store"
            packages = ["bashInteractive", "coreutils"]
            command = ["bash", "-l"]
        "#;
        let cfg = parse_toml(toml, "dev").unwrap();
        let rt = cfg.config.unwrap().runtime.unwrap();
        assert_eq!(rt.mode.as_deref(), Some("rootfs-store"));
        assert_eq!(
            rt.packages.as_deref(),
            Some(&["bashInteractive".to_string(), "coreutils".to_string()][..])
        );
        assert_eq!(
            rt.command.as_deref(),
            Some(&["bash".to_string(), "-l".to_string()][..])
        );
    }

    #[test]
    fn parses_attach_section() {
        let toml = r#"
            [config.attach]
            shell = "/bin/bash"
            tmuxSession = "main"
            startDelay = "500ms"
        "#;
        let cfg = parse_toml(toml, "dev").unwrap();
        let attach = cfg.config.unwrap().attach.unwrap();
        assert_eq!(attach.shell.as_deref(), Some("/bin/bash"));
        assert_eq!(attach.tmux_session.as_deref(), Some("main"));
        assert_eq!(attach.start_delay.as_deref(), Some("500ms"));
    }

    #[test]
    fn parses_home_section() {
        let toml = r#"
            [config.home]
            mode = "persistent"
            source = "~/.graft/devshell"
            target = "/home/user"
        "#;
        let cfg = parse_toml(toml, "dev").unwrap();
        let home = cfg.config.unwrap().home.unwrap();
        assert_eq!(home.mode.as_deref(), Some("persistent"));
        assert_eq!(home.source.as_deref(), Some("~/.graft/devshell"));
        assert_eq!(home.target.as_deref(), Some("/home/user"));
    }

    #[test]
    fn unknown_field_returns_error() {
        let result = parse_toml(r#"unknown_field = "oops""#, "x");
        assert!(result.is_err());
    }

    #[test]
    fn malformed_toml_returns_error() {
        assert!(parse_toml("[[[ invalid", "x").is_err());
    }

    // ── load_from_path ────────────────────────────────────────────────────

    #[test]
    fn load_from_path_reads_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("mycontainer.toml");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, r"version = 1").unwrap();

        let cfg = load_from_path(&path, "mycontainer").unwrap();
        assert_eq!(cfg.name.as_deref(), Some("mycontainer"));
        assert_eq!(cfg.version, Some(1));
    }

    #[test]
    fn load_from_path_missing_file_returns_error() {
        let result = load_from_path(Path::new("/nonexistent/path/file.toml"), "x");
        assert!(result.is_err());
    }
}
