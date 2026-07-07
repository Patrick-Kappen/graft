//! Resolve user TOML config into the JSON spec consumed by Nix.

use anyhow::{bail, Result};
use serde::Serialize;

use crate::config::schema::{Container, ContainerConfig, DeployTarget, Runtime};

const SUPPORTED_VERSION: u32 = 1;
const GRAFT_PAUSE_PACKAGE: &str = "graft-pause";
const GRAFT_PAUSE_COMMAND: &str = "/bin/graft-pause";
const ROOTFS_STORE_MODE: &str = "rootfs-store";

/// Fully resolved container spec for the NixOS/Home Manager modules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedContainer {
    /// Container name.
    pub name: String,
    /// Deployment settings.
    pub deploy: ResolvedDeploy,
    /// Runtime settings used to build the rootfs and Quadlet `Exec=`.
    pub runtime: ResolvedRuntime,
    /// Optional container settings rendered into Quadlet `[Container]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<ResolvedContainerSettings>,
    /// Optional systemd service settings.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub service: Option<ResolvedService>,
}

/// Resolved deployment settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedDeploy {
    /// Whether the module should render this container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enable: Option<bool>,
    /// System or user Quadlet target.
    pub target: ResolvedDeployTarget,
}

/// Resolved deployment target.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResolvedDeployTarget {
    /// Rootful/system Quadlet container.
    System,
    /// Rootless/user Quadlet container.
    User,
}

/// Resolved runtime settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedRuntime {
    /// Runtime mode. Currently always `rootfs-store`.
    pub mode: String,
    /// Nix packages to include in the rootfs.
    pub packages: Vec<String>,
    /// Command rendered as Quadlet `Exec=`.
    pub command: Vec<String>,
}

/// Resolved container settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedContainerSettings {
    /// Optional hostname rendered as Quadlet `HostName=`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hostname: Option<String>,
}

/// Resolved service settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedService {
    /// Optional systemd restart policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart: Option<String>,
}

/// Resolve a parsed TOML config into the JSON-ready container spec.
///
/// # Errors
///
/// Returns an error when required fields are missing or unsupported values are
/// present.
pub fn resolve(config: &ContainerConfig) -> Result<ResolvedContainer> {
    validate_version(config)?;

    let name = resolve_name(config)?;
    let runtime = config
        .config
        .as_ref()
        .and_then(|config| config.runtime.as_ref());

    validate_runtime_mode(runtime)?;

    Ok(ResolvedContainer {
        name,
        deploy: ResolvedDeploy {
            enable: config.deploy.as_ref().and_then(|deploy| deploy.enable),
            target: resolve_deploy_target(
                config
                    .deploy
                    .as_ref()
                    .and_then(|deploy| deploy.target.as_ref()),
            ),
        },
        runtime: ResolvedRuntime {
            mode: ROOTFS_STORE_MODE.to_string(),
            packages: resolve_packages(runtime)?,
            command: resolve_command(runtime)?,
        },
        container: resolve_container(config)?,
        service: resolve_service(config)?,
    })
}

fn resolve_container(config: &ContainerConfig) -> Result<Option<ResolvedContainerSettings>> {
    let container = config
        .config
        .as_ref()
        .and_then(|config| config.container.as_ref());
    let hostname = resolve_hostname(container)?;

    if hostname.is_none() {
        return Ok(None);
    }

    Ok(Some(ResolvedContainerSettings { hostname }))
}

fn resolve_hostname(container: Option<&Container>) -> Result<Option<String>> {
    let Some(hostname) = container.and_then(|container| container.hostname.as_ref()) else {
        return Ok(None);
    };

    validate_hostname(hostname)?;

    Ok(Some(hostname.clone()))
}

fn validate_hostname(hostname: &str) -> Result<()> {
    if hostname.trim().is_empty() {
        bail!("container hostname cannot be empty");
    }

    if hostname.chars().any(char::is_control) {
        bail!("container hostname cannot contain control characters");
    }

    Ok(())
}

fn validate_version(config: &ContainerConfig) -> Result<()> {
    match config.version {
        Some(SUPPORTED_VERSION) => Ok(()),
        Some(version) => bail!("unsupported config version: {version}"),
        None => bail!("config version is required"),
    }
}

fn resolve_name(config: &ContainerConfig) -> Result<String> {
    let Some(name) = config.name.as_ref() else {
        bail!("container name is required");
    };

    if name.trim().is_empty() {
        bail!("container name cannot be empty");
    }

    if !is_safe_container_name(name) {
        bail!("container name contains unsupported characters");
    }

    Ok(name.clone())
}

fn is_safe_container_name(name: &str) -> bool {
    let mut chars = name.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    first.is_ascii_alphanumeric()
        && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
}

fn validate_runtime_mode(runtime: Option<&Runtime>) -> Result<()> {
    let Some(mode) = runtime.and_then(|runtime| runtime.mode.as_deref()) else {
        return Ok(());
    };

    if mode == ROOTFS_STORE_MODE {
        Ok(())
    } else {
        bail!("unsupported runtime mode: {mode}");
    }
}

fn resolve_deploy_target(target: Option<&DeployTarget>) -> ResolvedDeployTarget {
    match target {
        Some(DeployTarget::User) => ResolvedDeployTarget::User,
        Some(DeployTarget::System) | None => ResolvedDeployTarget::System,
    }
}

fn resolve_packages(runtime: Option<&Runtime>) -> Result<Vec<String>> {
    let mut packages = Vec::new();
    push_unique(&mut packages, GRAFT_PAUSE_PACKAGE);

    if let Some(user_packages) = runtime.and_then(|runtime| runtime.packages.as_ref()) {
        for package in user_packages {
            validate_package_name(package)?;
            push_unique(&mut packages, package);
        }
    }

    Ok(packages)
}

fn validate_package_name(package: &str) -> Result<()> {
    if package.is_empty() {
        bail!("runtime package names cannot be empty");
    }

    if package
        .chars()
        .any(|ch| ch.is_control() || ch.is_whitespace())
    {
        bail!("runtime package names cannot contain whitespace or control characters");
    }

    Ok(())
}

fn resolve_command(runtime: Option<&Runtime>) -> Result<Vec<String>> {
    let Some(command) = runtime.and_then(|runtime| runtime.command.as_ref()) else {
        return Ok(vec![GRAFT_PAUSE_COMMAND.to_string()]);
    };

    if command.is_empty() {
        bail!("runtime command cannot be empty");
    }

    for arg in command {
        validate_command_arg(arg)?;
    }

    Ok(command.clone())
}

fn validate_command_arg(arg: &str) -> Result<()> {
    if arg.is_empty() {
        bail!("runtime command entries cannot be empty");
    }

    if arg.chars().any(char::is_control) {
        bail!("runtime command entries cannot contain control characters");
    }

    Ok(())
}

fn resolve_service(config: &ContainerConfig) -> Result<Option<ResolvedService>> {
    let restart = config
        .config
        .as_ref()
        .and_then(|config| config.service.as_ref())
        .and_then(|service| service.restart.as_ref());

    let Some(restart) = restart else {
        return Ok(None);
    };

    validate_restart_policy(restart)?;

    Ok(Some(ResolvedService {
        restart: Some(restart.clone()),
    }))
}

fn validate_restart_policy(restart: &str) -> Result<()> {
    if restart.chars().any(char::is_control) {
        bail!("restart policy cannot contain control characters");
    }

    if matches!(
        restart,
        "no" | "on-success" | "on-failure" | "on-abnormal" | "on-watchdog" | "on-abort" | "always"
    ) {
        Ok(())
    } else {
        bail!("unsupported restart policy: {restart}");
    }
}

fn push_unique(packages: &mut Vec<String>, package: &str) {
    if !packages.iter().any(|existing| existing == package) {
        packages.push(package.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use crate::config::schema::{Config, Deploy, Service};

    use super::*;

    fn named_config() -> ContainerConfig {
        ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            name: Some("dev".to_string()),
            ..ContainerConfig::default()
        }
    }

    fn runtime_config(runtime: Runtime) -> ContainerConfig {
        ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            name: Some("dev".to_string()),
            config: Some(Config {
                runtime: Some(runtime),
                ..Config::default()
            }),
            ..ContainerConfig::default()
        }
    }

    fn container_config(container: Container) -> ContainerConfig {
        ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            name: Some("dev".to_string()),
            config: Some(Config {
                container: Some(container),
                ..Config::default()
            }),
            ..ContainerConfig::default()
        }
    }

    #[test]
    fn missing_version_returns_error() {
        let config = ContainerConfig {
            name: Some("dev".to_string()),
            ..ContainerConfig::default()
        };

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn unsupported_version_returns_error() {
        let config = ContainerConfig {
            version: Some(2),
            name: Some("dev".to_string()),
            ..ContainerConfig::default()
        };

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn missing_name_returns_error() {
        let config = ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            ..ContainerConfig::default()
        };

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn empty_name_returns_error() {
        let config = ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            name: Some("  ".to_string()),
            ..ContainerConfig::default()
        };
        let result = resolve(&config);
        assert!(result.is_err());
    }

    #[test]
    fn unsafe_name_returns_error() {
        let config = ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            name: Some("bad/name".to_string()),
            ..ContainerConfig::default()
        };

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn default_command_is_graft_pause() {
        let resolved = resolve(&named_config()).unwrap();
        assert_eq!(resolved.runtime.command, [GRAFT_PAUSE_COMMAND]);
    }

    #[test]
    fn user_command_is_preserved() {
        let config = runtime_config(Runtime {
            command: Some(vec!["node".to_string(), "server.js".to_string()]),
            ..Runtime::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(resolved.runtime.command, ["node", "server.js"]);
    }

    #[test]
    fn empty_user_command_returns_error() {
        let config = runtime_config(Runtime {
            command: Some(Vec::new()),
            ..Runtime::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn empty_user_command_entry_returns_error() {
        let config = runtime_config(Runtime {
            command: Some(vec!["node".to_string(), String::new()]),
            ..Runtime::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn control_character_in_command_entry_returns_error() {
        let config = runtime_config(Runtime {
            command: Some(vec!["node".to_string(), "server\n.js".to_string()]),
            ..Runtime::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn graft_pause_is_always_in_packages() {
        let config = runtime_config(Runtime {
            packages: Some(vec!["nodejs".to_string()]),
            ..Runtime::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(resolved.runtime.packages, ["graft-pause", "nodejs"]);
    }

    #[test]
    fn graft_pause_is_not_duplicated() {
        let config = runtime_config(Runtime {
            packages: Some(vec![
                "graft-pause".to_string(),
                "nodejs".to_string(),
                "nodejs".to_string(),
            ]),
            ..Runtime::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(resolved.runtime.packages, ["graft-pause", "nodejs"]);
    }

    #[test]
    fn empty_package_name_returns_error() {
        let config = runtime_config(Runtime {
            packages: Some(vec![String::new()]),
            ..Runtime::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn whitespace_in_package_name_returns_error() {
        let config = runtime_config(Runtime {
            packages: Some(vec!["bad package".to_string()]),
            ..Runtime::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn hostname_has_no_default() {
        let resolved = resolve(&named_config()).unwrap();
        assert_eq!(resolved.container, None);
    }

    #[test]
    fn explicit_hostname_is_preserved() {
        let config = container_config(Container {
            hostname: Some("web.local".to_string()),
            ..Container::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.container,
            Some(ResolvedContainerSettings {
                hostname: Some("web.local".to_string()),
            })
        );

        let json = serde_json::to_value(&resolved).unwrap();
        assert_eq!(json["container"]["hostname"], "web.local");
    }

    #[test]
    fn empty_hostname_returns_error() {
        let config = container_config(Container {
            hostname: Some("  ".to_string()),
            ..Container::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn control_character_in_hostname_returns_error() {
        let config = container_config(Container {
            hostname: Some("web\nlocal".to_string()),
            ..Container::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn restart_has_no_default() {
        let resolved = resolve(&named_config()).unwrap();
        assert_eq!(resolved.service, None);
    }

    #[test]
    fn explicit_restart_is_preserved() {
        let config = ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            name: Some("dev".to_string()),
            config: Some(Config {
                service: Some(Service {
                    restart: Some("on-failure".to_string()),
                    ..Service::default()
                }),
                ..Config::default()
            }),
            ..ContainerConfig::default()
        };

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.service,
            Some(ResolvedService {
                restart: Some("on-failure".to_string()),
            })
        );
    }

    #[test]
    fn all_supported_restart_policies_are_accepted() {
        for restart in [
            "no",
            "on-success",
            "on-failure",
            "on-abnormal",
            "on-watchdog",
            "on-abort",
            "always",
        ] {
            let config = ContainerConfig {
                version: Some(SUPPORTED_VERSION),
                name: Some("dev".to_string()),
                config: Some(Config {
                    service: Some(Service {
                        restart: Some(restart.to_string()),
                        ..Service::default()
                    }),
                    ..Config::default()
                }),
                ..ContainerConfig::default()
            };

            assert!(resolve(&config).is_ok(), "{restart} is accepted");
        }
    }

    #[test]
    fn unsupported_restart_policy_returns_error() {
        let config = ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            name: Some("dev".to_string()),
            config: Some(Config {
                service: Some(Service {
                    restart: Some("unless-stopped".to_string()),
                    ..Service::default()
                }),
                ..Config::default()
            }),
            ..ContainerConfig::default()
        };

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn deploy_enable_has_no_default() {
        let resolved = resolve(&named_config()).unwrap();
        assert_eq!(resolved.deploy.enable, None);
    }

    #[test]
    fn explicit_deploy_enable_is_preserved() {
        let config = ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            name: Some("dev".to_string()),
            deploy: Some(Deploy {
                enable: Some(false),
                ..Deploy::default()
            }),
            ..ContainerConfig::default()
        };

        let resolved = resolve(&config).unwrap();

        assert_eq!(resolved.deploy.enable, Some(false));
    }

    #[test]
    fn deploy_target_defaults_to_system() {
        let resolved = resolve(&named_config()).unwrap();
        assert_eq!(resolved.deploy.target, ResolvedDeployTarget::System);
    }

    #[test]
    fn explicit_user_target_is_preserved() {
        let config = ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            name: Some("dev".to_string()),
            deploy: Some(Deploy {
                target: Some(DeployTarget::User),
                ..Deploy::default()
            }),
            ..ContainerConfig::default()
        };

        let resolved = resolve(&config).unwrap();

        assert_eq!(resolved.deploy.target, ResolvedDeployTarget::User);
    }

    #[test]
    fn runtime_mode_defaults_to_rootfs_store() {
        let resolved = resolve(&named_config()).unwrap();
        assert_eq!(resolved.runtime.mode, ROOTFS_STORE_MODE);
    }

    #[test]
    fn rootfs_store_runtime_mode_is_supported() {
        let config = runtime_config(Runtime {
            mode: Some(ROOTFS_STORE_MODE.to_string()),
            ..Runtime::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(resolved.runtime.mode, ROOTFS_STORE_MODE);
    }

    #[test]
    fn invalid_runtime_mode_returns_error() {
        let config = runtime_config(Runtime {
            mode: Some("image".to_string()),
            ..Runtime::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn omits_unset_optional_fields_from_json() {
        let resolved = resolve(&named_config()).unwrap();
        let json = serde_json::to_value(&resolved).unwrap();

        assert_eq!(json.get("container"), None);
        assert_eq!(json.get("service"), None);
        assert_eq!(json["deploy"].get("enable"), None);
    }
}
