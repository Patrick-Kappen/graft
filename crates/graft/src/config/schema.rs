//! Complete TOML schema for graft container configuration.
//!
//! All fields are `Option<_>` so that partial configs (used as override layers)
//! parse successfully and can be merged with a base layer later.
//! Validation of required fields and cross-field constraints (e.g.
//! `config.container.group` requires `config.container.user`) happens at
//! use-time, not at parse-time.
//!
//! The generated JSON Schema describes complete, currently supported workload
//! definitions. Schema-only skips intentionally hide reserved parser fields.

use schemars::JsonSchema;
use serde::{de, Deserialize, Deserializer};
use std::collections::HashMap;

/// Top-level container configuration.
#[derive(Debug, Clone, Deserialize, Default, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
#[schemars(
    title = "Graft container configuration v1",
    description = "Supported Graft TOML workload intent for schema version 1.",
    extend("$id" = "urn:graft:schema:v1")
)]
pub struct ContainerConfig {
    /// Schema version. Must be `1`.
    #[schemars(required, range(min = 1, max = 1))]
    pub version: Option<u32>,
    /// Container and Podman identity. Keep it equal to the TOML filename stem
    /// until the final unit identity contract is implemented.
    #[schemars(required, regex(pattern = r"^[A-Za-z0-9][A-Za-z0-9._-]*$"))]
    pub name: Option<String>,
    /// Parent graph nodes to inherit from.
    #[schemars(skip)]
    pub parents: Option<GraphRefs>,
    /// Child graph nodes that inherit from this node.
    #[schemars(skip)]
    pub children: Option<GraphRefs>,
    /// Typed relationships to Graft workloads or explicit external units.
    pub dependencies: Option<Vec<Dependency>>,
    /// Module deployment settings.
    pub deploy: Option<Deploy>,
    /// Validation behaviour.
    #[schemars(skip)]
    pub validation: Option<Validation>,
    /// Container runtime and platform configuration.
    pub config: Option<Config>,
}

/// `add` / `remove` / `set` list of graph-node refs (`[parents]` / `[children]`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct GraphRefs {
    pub add: Option<Vec<String>>,
    pub remove: Option<Vec<String>>,
    pub set: Option<Vec<String>>,
}

/// One typed workload or external-unit relationship (`[[dependencies]]`).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Dependency {
    /// Exactly one typed relationship target.
    pub target: DependencyTarget,
    /// Optional activation and failure requirement.
    pub requirement: Option<DependencyRequirement>,
    /// Optional startup ordering relationship.
    pub ordering: Option<DependencyOrdering>,
    /// Optional stop/restart lifecycle coupling.
    pub lifecycle: Option<DependencyLifecycle>,
}

/// Typed target for a dependency relationship.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(untagged)]
pub enum DependencyTarget {
    /// Another workload in the explicit Graft source set.
    Workload(WorkloadDependencyTarget),
    /// An exact unit in the selected system or user manager.
    ExternalUnit(ExternalUnitDependencyTarget),
}

/// Graft workload dependency target.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct WorkloadDependencyTarget {
    /// Safe top-level Graft workload name.
    #[schemars(regex(pattern = r"^[A-Za-z0-9][A-Za-z0-9._-]*$"))]
    pub workload: String,
}

/// External systemd unit dependency target.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct ExternalUnitDependencyTarget {
    /// Concrete systemd unit name in the selected manager.
    #[schemars(length(min = 1, max = 255))]
    pub external_unit: String,
}

/// Activation and failure coupling for a dependency.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DependencyRequirement {
    /// Start the target and couple activation failure through `Requires=`.
    Required,
    /// Start the target without coupling activation failure through `Wants=`.
    Optional,
}

/// Ordering relative to a dependency target.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DependencyOrdering {
    /// Start after the target's start job completes.
    After,
    /// Start before the target.
    Before,
}

/// Stop and restart lifecycle coupling for a dependency.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum DependencyLifecycle {
    /// Propagate target stop and restart operations through `PartOf=`.
    PartOf,
    /// Bind active state to the target through `BindsTo=`.
    Bound,
}

/// Module deployment settings (`[deploy]`).
#[derive(Debug, Clone, Deserialize, Default, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Deploy {
    /// Whether the NixOS / HM module should render this container.
    pub enable: Option<bool>,
    /// Scope to render the Quadlet unit in.
    pub target: Option<DeployTarget>,
    /// Optional service-manager startup activation.
    pub activation: Option<DeployActivation>,
}

/// Supported deployment activation intent.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DeployActivation {
    /// Request the workload during normal target-manager startup.
    Startup,
}

/// Deploy scope.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum DeployTarget {
    /// NixOS system manager and rootful Podman.
    System,
    /// Home Manager user manager; Podman is rootless only for a non-root account.
    User,
}

/// Validation settings (`[validation]`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Validation {
    pub level: Option<ValidationLevel>,
}

/// Validation strictness level.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ValidationLevel {
    Off,
    Warn,
    Strict,
}

/// All container configuration (`[config]`).
#[derive(Debug, Clone, Deserialize, Default, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Config {
    /// Runtime packages and process.
    pub runtime: Option<Runtime>,
    /// Supported Quadlet container settings.
    pub container: Option<Container>,
    /// Supported tmpfs, filesystem volume, and CDI device settings.
    pub filesystem: Option<Filesystem>,
    /// Supported network namespace and published-port settings.
    pub network: Option<Network>,
    /// Extra Quadlet `.network` units (`[[config.networks]]`).
    #[schemars(skip)]
    pub networks: Option<Vec<NetworkUnit>>,
    /// Extra Quadlet `.volume` units (`[[config.volumes]]`).
    #[schemars(skip)]
    pub volumes: Option<Vec<VolumeUnit>>,
    /// Explicit non-relaxing container hardening controls.
    pub security: Option<Security>,
    #[schemars(skip)]
    pub resources: Option<Resources>,
    #[schemars(skip)]
    pub secrets: Option<Vec<Secret>>,
    #[schemars(skip)]
    pub workspace: Option<Workspace>,
    #[schemars(skip)]
    pub home: Option<Home>,
    #[schemars(skip)]
    pub attach: Option<Attach>,
    /// Supported systemd service settings.
    pub service: Option<Service>,
    #[schemars(skip)]
    pub quadlet: Option<Quadlet>,
}

/// Runtime configuration (`[config.runtime]`).
#[derive(Debug, Clone, Deserialize, Default, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Runtime {
    /// Only `"rootfs-store"` is supported today.
    #[schemars(regex(pattern = r"^rootfs-store$"))]
    pub mode: Option<String>,
    /// Nix packages realised onto the container PATH.
    #[schemars(inner(length(min = 1)))]
    pub packages: Option<Vec<String>>,
    /// The non-empty process argument vector to run inside the container.
    #[schemars(length(min = 1), inner(length(min = 1)))]
    pub command: Option<Vec<String>>,
    /// Package mutations applied after the graph merge (module-only).
    #[schemars(skip)]
    pub package_ops: Option<PackageOps>,
}

/// Package-level mutations (`[config.runtime.packageOps]`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PackageOps {
    pub add: Option<Vec<String>>,
    pub remove: Option<Vec<String>>,
    pub replace: Option<Vec<PackageReplace>>,
}

/// A single package replacement entry.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct PackageReplace {
    pub name: String,
    pub with: String,
}

/// Container settings (`[config.container]`).
#[derive(Debug, Clone, Deserialize, Default, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Container {
    #[schemars(skip)]
    pub name: Option<String>,
    /// Literal Quadlet `HostName=` value.
    #[schemars(length(min = 1))]
    pub hostname: Option<String>,
    #[schemars(skip)]
    pub pod: Option<String>,
    #[schemars(skip)]
    pub entrypoint: Option<Vec<String>>,
    #[schemars(skip)]
    pub stop_signal: Option<String>,
    #[schemars(skip)]
    pub stop_timeout: Option<u32>,
    /// Existing process working directory inside the container.
    #[schemars(length(min = 1))]
    pub working_dir: Option<String>,
    /// Literal Quadlet `User=` value.
    #[schemars(length(min = 1))]
    pub user: Option<String>,
    /// Literal Quadlet `Group=` value; requires `user` during resolution.
    #[schemars(length(min = 1))]
    pub group: Option<String>,
    #[schemars(skip)]
    pub timezone: Option<String>,
    #[schemars(skip)]
    pub notify: Option<String>,
    #[schemars(skip)]
    pub run_init: Option<bool>,
    #[schemars(skip)]
    pub annotations: Option<HashMap<String, String>>,
    /// Environment assignments rendered in sorted key order.
    pub environment: Option<HashMap<String, String>>,
    /// Ordered literal Quadlet `EnvironmentFile=` paths.
    #[schemars(inner(length(min = 1)))]
    pub environment_file: Option<Vec<String>>,
    #[schemars(skip)]
    pub environment_host: Option<bool>,
    #[schemars(skip)]
    pub podman_args: Option<Vec<String>>,
    #[schemars(skip)]
    pub global_args: Option<Vec<String>>,
    #[schemars(skip)]
    pub ip: Option<String>,
    #[schemars(skip)]
    pub ip6: Option<String>,
    #[schemars(skip)]
    pub network_alias: Option<Vec<String>>,
    #[schemars(skip)]
    pub expose_host_port: Option<Vec<String>>,
    #[schemars(skip)]
    pub uid_map: Option<Vec<String>>,
    #[schemars(skip)]
    pub gid_map: Option<Vec<String>>,
    #[schemars(skip)]
    pub sub_uid_map: Option<String>,
    #[schemars(skip)]
    pub sub_gid_map: Option<String>,
    #[schemars(skip)]
    pub shm_size: Option<String>,
    #[schemars(skip)]
    pub mask: Option<Vec<String>>,
    #[schemars(skip)]
    pub unmask_paths: Option<Vec<String>>,
    #[schemars(skip)]
    pub sysctl: Option<Vec<String>>,
    #[schemars(skip)]
    pub log_driver: Option<String>,
    #[schemars(skip)]
    pub health: Option<Health>,
}

/// Container health check (`[config.container.health]`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Health {
    pub cmd: Option<String>,
    pub interval: Option<String>,
    pub timeout: Option<String>,
    pub retries: Option<u32>,
    pub start_period: Option<String>,
    pub on_failure: Option<String>,
    pub startup_cmd: Option<String>,
    pub startup_interval: Option<String>,
    pub startup_retries: Option<u32>,
    pub startup_success: Option<u32>,
    pub startup_timeout: Option<String>,
}

/// Filesystem configuration (`[config.filesystem]`).
#[derive(Debug, Clone, Deserialize, Default, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Filesystem {
    /// Make the container root filesystem read-only. Only `true` is supported.
    #[schemars(extend("const" = true))]
    pub read_only: Option<bool>,
    #[schemars(skip)]
    pub read_only_tmpfs: Option<bool>,
    /// Ordered absolute container paths backed by writable tmpfs mounts.
    #[schemars(
        inner(regex(pattern = r"^/[^:\u0000-\u001F\u007F]*$")),
        extend("uniqueItems" = true)
    )]
    pub tmpfs: Option<Vec<String>>,
    /// Raw mount strings passed to `--mount`.
    #[schemars(skip)]
    pub mounts: Option<Vec<String>>,
    /// Ordered literal Quadlet `Volume=` entries.
    pub volumes: Option<Vec<FilesystemVolume>>,
    /// Ordered qualified CDI device references.
    pub devices: Option<Vec<Device>>,
}

/// A single volume mount (`[[config.filesystem.volumes]]`).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct FilesystemVolume {
    /// Optional volume source. Colons are not supported.
    #[schemars(length(min = 1))]
    pub source: Option<String>,
    /// Required container target. Colons are not supported.
    #[schemars(length(min = 1))]
    pub target: String,
    /// Optional volume mode or options. Requires `source`; colons are not
    /// supported.
    #[schemars(length(min = 1))]
    pub mode: Option<String>,
}

/// A qualified CDI device reference (`[[config.filesystem.devices]]`).
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Device {
    /// Colon-free CDI qualified name in `vendor/class=device` form.
    #[schemars(regex(
        pattern = r"^[A-Za-z][A-Za-z0-9._-]*[A-Za-z0-9]/[A-Za-z][A-Za-z0-9._-]*[A-Za-z0-9]=[A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9])?$"
    ))]
    pub source: String,
    /// Reserved direct-device target remapping.
    #[schemars(skip)]
    pub target: Option<String>,
    /// Reserved direct-device permissions.
    #[schemars(skip)]
    pub permissions: Option<String>,
}

/// Network configuration (`[config.network]`).
#[derive(Debug, Clone, Deserialize, Default, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Network {
    /// Network namespace intent. Absence preserves Quadlet's default.
    pub mode: Option<NetworkMode>,
    /// Graft workload whose network namespace should be shared.
    #[schemars(regex(pattern = r"^[A-Za-z0-9][A-Za-z0-9._-]*$"))]
    pub container: Option<String>,
    /// Ordered literal Quadlet `PublishPort=` entries.
    #[schemars(inner(length(min = 1)))]
    pub publish: Option<Vec<String>>,
    #[schemars(skip)]
    pub dns: Option<Vec<String>>,
    #[schemars(skip)]
    pub dns_option: Option<Vec<String>>,
    #[schemars(skip)]
    pub dns_search: Option<Vec<String>>,
    #[schemars(skip)]
    pub add_host: Option<Vec<String>>,
}

/// Supported network namespace intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, JsonSchema)]
#[schemars(rename_all = "lowercase")]
pub enum NetworkMode {
    /// Create no externally connected IP network for the workload.
    None,
    /// Share another Graft workload's network namespace.
    Container,
}

impl<'de> Deserialize<'de> for NetworkMode {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = String::deserialize(deserializer)?;
        match value.as_str() {
            "none" => Ok(Self::None),
            "container" => Ok(Self::Container),
            value if value.starts_with("container:") => Err(de::Error::custom(format!(
                "config.network.mode = {value:?} is not supported; use config.network.mode = \
                 \"container\" with config.network.container = {:?}",
                value.trim_start_matches("container:")
            ))),
            "host" => Err(de::Error::custom(
                "config.network.mode = \"host\" is dangerous and not supported yet",
            )),
            _ => Err(de::Error::unknown_variant(&value, &["none", "container"])),
        }
    }
}

/// Extra Quadlet `.network` unit (`[[config.networks]]`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct NetworkUnit {
    pub name: Option<String>,
    pub driver: Option<String>,
    pub internal: Option<bool>,
    pub ipv6: Option<bool>,
    pub subnet: Option<String>,
    pub gateway: Option<String>,
    pub ip_range: Option<String>,
    pub dns: Option<Vec<String>>,
    pub options: Option<String>,
    pub labels: Option<HashMap<String, String>>,
    pub quadlet: Option<HashMap<String, Vec<String>>>,
}

/// Extra Quadlet `.volume` unit (`[[config.volumes]]`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct VolumeUnit {
    pub name: Option<String>,
    pub driver: Option<String>,
    pub copy: Option<bool>,
    pub options: Option<String>,
    pub labels: Option<HashMap<String, String>>,
    pub quadlet: Option<HashMap<String, Vec<String>>>,
}

/// Security settings (`[config.security]`).
#[derive(Debug, Clone, Deserialize, Default, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Security {
    /// Ordered non-empty list of `all` or canonical `CAP_*` capability names.
    #[schemars(
        length(min = 1),
        inner(regex(pattern = r"^(all|CAP_[A-Z][A-Z0-9_]*)$"))
    )]
    pub drop_capabilities: Option<Vec<String>>,
    #[schemars(skip)]
    pub add_capabilities: Option<Vec<String>>,
    /// Prevent processes from gaining privileges. Only `true` is supported.
    #[schemars(extend("const" = true))]
    pub no_new_privileges: Option<bool>,
    #[schemars(skip)]
    pub privileged: Option<bool>,
    #[schemars(skip)]
    pub seccomp_profile: Option<String>,
    #[schemars(skip)]
    pub security_label_disable: Option<bool>,
    #[schemars(skip)]
    pub security_label_file_type: Option<String>,
    #[schemars(skip)]
    pub security_label_level: Option<String>,
    #[schemars(skip)]
    pub security_label_nested: Option<bool>,
    #[schemars(skip)]
    pub security_label_type: Option<String>,
    #[schemars(skip)]
    pub security_opt: Option<Vec<String>>,
    #[schemars(skip)]
    pub userns: Option<String>,
}

/// Resource limits (`[config.resources]`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Resources {
    pub memory: Option<String>,
    pub memory_swap: Option<String>,
    pub cpus: Option<String>,
    pub cpu_quota: Option<String>,
    pub pids_limit: Option<i64>,
    pub ulimits: Option<Vec<String>>,
}

/// A secret reference (`[[config.secrets]]`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Secret {
    pub name: String,
    pub target: Option<String>,
    #[serde(rename = "type")]
    pub secret_type: Option<String>,
    pub uid: Option<String>,
    pub gid: Option<String>,
    pub mode: Option<String>,
    pub options: Option<String>,
}

/// Workspace isolation settings (`[config.workspace]`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Workspace {
    /// `"none"` (default) or `"copy"`.
    pub mode: Option<String>,
    pub source: Option<String>,
    pub target: Option<String>,
    pub review: Option<String>,
    pub promote: Option<String>,
    pub exclude_patterns: Option<Vec<String>>,
}

/// Home directory isolation settings (`[config.home]`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Home {
    /// `"ephemeral"`, `"persistent"`, or `"session"`.
    pub mode: Option<String>,
    pub source: Option<String>,
    pub target: Option<String>,
    pub review: Option<String>,
    pub promote: Option<String>,
    /// Legacy alias for `mode = "ephemeral"`.
    pub ephemeral: Option<bool>,
    /// Extra isolated paths (`[[config.home.shadow]]`).
    pub shadow: Option<Vec<HomeShadow>>,
}

/// A shadow mount entry (`[[config.home.shadow]]`).
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct HomeShadow {
    pub container: String,
    pub host: String,
}

/// Attach / shell settings (`[config.attach]`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Attach {
    pub tmux_session: Option<String>,
    pub shell: Option<String>,
    pub start_delay: Option<String>,
}

/// User-facing workload lifecycle.
#[derive(Debug, Clone, Copy, Deserialize, PartialEq, Eq, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum ServiceLifecycle {
    /// Continuously available process using Quadlet's notify lifecycle.
    LongRunning,
    /// Finite, repeatable process that becomes inactive after success.
    Job,
    /// Finite process that remains active/exited after success.
    Setup,
}

/// systemd service settings (`[config.service]`).
#[derive(Debug, Clone, Deserialize, Default, JsonSchema)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Service {
    /// Typed workload lifecycle. Defaults to `long-running` when absent.
    pub lifecycle: Option<ServiceLifecycle>,
    /// Reserved raw systemd service type; use `lifecycle` instead.
    #[serde(rename = "type")]
    #[schemars(skip)]
    pub service_type: Option<String>,
    /// Explicit systemd restart policy.
    #[schemars(regex(
        pattern = r"^(no|on-success|on-failure|on-abnormal|on-watchdog|on-abort|always)$"
    ))]
    pub restart: Option<String>,
    /// Literal systemd restart delay.
    #[schemars(length(min = 1))]
    pub restart_sec: Option<String>,
    /// Literal systemd start timeout.
    #[schemars(length(min = 1))]
    pub timeout_start_sec: Option<String>,
    /// Literal systemd stop timeout.
    #[schemars(length(min = 1))]
    pub timeout_stop_sec: Option<String>,
    /// Reserved raw systemd state retention; use `lifecycle` instead.
    #[schemars(skip)]
    pub remain_after_exit: Option<bool>,
    #[schemars(skip)]
    pub restart_if_changed: Option<bool>,
}

/// Raw Quadlet passthrough (`[config.quadlet]`).
#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct Quadlet {
    pub container: Option<HashMap<String, Vec<String>>>,
    pub service: Option<HashMap<String, Vec<String>>>,
    pub install: Option<HashMap<String, Vec<String>>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_toml(content: &str) -> Result<ContainerConfig, toml::de::Error> {
        toml::from_str(content)
    }

    #[test]
    fn empty_toml_is_valid() {
        let cfg = parse_toml("").unwrap();
        assert_eq!(cfg.name, None);
    }

    #[test]
    fn parses_version_name_and_deploy() {
        let toml = r#"
            version = 1
            name = "srv"

            [deploy]
            enable = true
            target = "user"
        "#;

        let cfg = parse_toml(toml).unwrap();

        assert_eq!(cfg.version, Some(1));
        assert_eq!(cfg.name.as_deref(), Some("srv"));
        let deploy = cfg.deploy.unwrap();
        assert_eq!(deploy.enable, Some(true));
        assert_eq!(deploy.target, Some(DeployTarget::User));
    }

    #[test]
    fn parses_startup_activation() {
        let cfg = parse_toml("[deploy]\nactivation = \"startup\"").unwrap();
        let activation = cfg.deploy.unwrap().activation;

        assert_eq!(activation, Some(DeployActivation::Startup));
    }

    #[test]
    fn parses_typed_workload_and_external_unit_dependencies() {
        let cfg = parse_toml(
            r#"
                [[dependencies]]
                target = { workload = "database" }
                requirement = "required"
                ordering = "after"
                lifecycle = "part-of"

                [[dependencies]]
                target = { externalUnit = "postgresql.service" }
                requirement = "optional"
                ordering = "before"
                lifecycle = "bound"
            "#,
        )
        .unwrap();
        let dependencies = cfg.dependencies.unwrap();

        assert_eq!(dependencies.len(), 2);
        assert!(matches!(
            &dependencies[0].target,
            DependencyTarget::Workload(target) if target.workload == "database"
        ));
        assert_eq!(
            dependencies[0].requirement,
            Some(DependencyRequirement::Required)
        );
        assert_eq!(dependencies[0].ordering, Some(DependencyOrdering::After));
        assert_eq!(dependencies[0].lifecycle, Some(DependencyLifecycle::PartOf));
        assert!(matches!(
            &dependencies[1].target,
            DependencyTarget::ExternalUnit(target)
                if target.external_unit == "postgresql.service"
        ));
        assert_eq!(
            dependencies[1].requirement,
            Some(DependencyRequirement::Optional)
        );
        assert_eq!(dependencies[1].ordering, Some(DependencyOrdering::Before));
        assert_eq!(dependencies[1].lifecycle, Some(DependencyLifecycle::Bound));
    }

    #[test]
    fn dependency_target_rejects_multiple_target_kinds() {
        let result = parse_toml(
            r#"
                [[dependencies]]
                target = { workload = "database", externalUnit = "database.service" }
                requirement = "required"
            "#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn unsupported_dependency_relation_returns_error() {
        let result = parse_toml(
            r#"
                [[dependencies]]
                target = { workload = "database" }
                requirement = "hard"
            "#,
        );

        assert!(result.is_err());
    }

    #[test]
    fn unsupported_deploy_activation_returns_error() {
        let result = parse_toml("[deploy]\nactivation = \"manual\"");

        assert!(result.is_err());
    }

    #[test]
    fn parses_runtime_section() {
        let toml = r#"
            [config.runtime]
            mode = "rootfs-store"
            packages = ["bashInteractive", "coreutils"]
            command = ["bash", "-l"]
        "#;

        let cfg = parse_toml(toml).unwrap();
        let runtime = cfg.config.unwrap().runtime.unwrap();

        assert_eq!(runtime.mode.as_deref(), Some("rootfs-store"));
        assert_eq!(
            runtime.packages.as_deref(),
            Some(&["bashInteractive".to_string(), "coreutils".to_string()][..])
        );
        assert_eq!(
            runtime.command.as_deref(),
            Some(&["bash".to_string(), "-l".to_string()][..])
        );
    }

    #[test]
    fn parses_explicit_hardening_controls() {
        let config = parse_toml(
            r#"
                [config.filesystem]
                readOnly = true

                [config.security]
                dropCapabilities = ["all"]
                noNewPrivileges = true
            "#,
        )
        .unwrap()
        .config
        .unwrap();

        assert_eq!(config.filesystem.unwrap().read_only, Some(true));
        let security = config.security.unwrap();
        assert_eq!(
            security.drop_capabilities.as_deref(),
            Some(&["all".to_string()][..])
        );
        assert_eq!(security.no_new_privileges, Some(true));
    }

    #[test]
    fn parses_cdi_device_source_and_reserved_direct_device_fields() {
        let config = parse_toml(
            r#"
                [[config.filesystem.devices]]
                source = "nvidia.com/gpu=all"
                target = "/dev/gpu0"
                permissions = "rwm"
            "#,
        )
        .unwrap();

        let devices = config.config.unwrap().filesystem.unwrap().devices.unwrap();

        assert_eq!(devices.len(), 1);
        assert_eq!(devices[0].source, "nvidia.com/gpu=all");
        assert_eq!(devices[0].target.as_deref(), Some("/dev/gpu0"));
        assert_eq!(devices[0].permissions.as_deref(), Some("rwm"));
    }

    #[test]
    fn parses_attach_section() {
        let toml = r#"
            [config.attach]
            shell = "/bin/bash"
            tmuxSession = "main"
            startDelay = "500ms"
        "#;

        let cfg = parse_toml(toml).unwrap();
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

        let cfg = parse_toml(toml).unwrap();
        let home = cfg.config.unwrap().home.unwrap();

        assert_eq!(home.mode.as_deref(), Some("persistent"));
        assert_eq!(home.source.as_deref(), Some("~/.graft/devshell"));
        assert_eq!(home.target.as_deref(), Some("/home/user"));
    }

    #[test]
    fn parses_supported_network_modes() {
        for (value, expected) in [
            ("none", NetworkMode::None),
            ("container", NetworkMode::Container),
        ] {
            let toml = format!("[config.network]\nmode = \"{value}\"");
            let cfg = parse_toml(&toml).unwrap();
            let mode = cfg.config.unwrap().network.unwrap().mode;

            assert_eq!(mode, Some(expected));
        }
    }

    #[test]
    fn old_container_network_mode_returns_migration_error() {
        let error = parse_toml("[config.network]\nmode = \"container:database\"").unwrap_err();

        assert!(error.to_string().contains(
            "use config.network.mode = \"container\" with config.network.container = \"database\""
        ));
    }

    #[test]
    fn dangerous_host_network_mode_returns_error() {
        let error = parse_toml("[config.network]\nmode = \"host\"").unwrap_err();

        assert!(error
            .to_string()
            .contains("config.network.mode = \"host\" is dangerous and not supported yet"));
    }

    #[test]
    fn parses_supported_service_lifecycles() {
        for (value, expected) in [
            ("long-running", ServiceLifecycle::LongRunning),
            ("job", ServiceLifecycle::Job),
            ("setup", ServiceLifecycle::Setup),
        ] {
            let toml = format!("[config.service]\nlifecycle = \"{value}\"");
            let cfg = parse_toml(&toml).unwrap();
            let lifecycle = cfg.config.unwrap().service.unwrap().lifecycle;

            assert_eq!(lifecycle, Some(expected));
        }
    }

    #[test]
    fn parses_raw_service_fields_for_migration_diagnostics() {
        let cfg =
            parse_toml("[config.service]\ntype = \"oneshot\"\nremainAfterExit = false").unwrap();
        let service = cfg.config.unwrap().service.unwrap();

        assert_eq!(service.service_type.as_deref(), Some("oneshot"));
        assert_eq!(service.remain_after_exit, Some(false));
    }

    #[test]
    fn unsupported_service_lifecycle_returns_error() {
        let result = parse_toml("[config.service]\nlifecycle = \"oneshot\"");

        assert!(result.is_err());
    }

    #[test]
    fn unknown_field_returns_error() {
        let result = parse_toml(r#"unknown_field = "oops""#);
        assert!(result.is_err());
    }

    #[test]
    fn malformed_toml_returns_error() {
        let result = parse_toml("[[[ invalid");
        assert!(result.is_err());
    }
}
