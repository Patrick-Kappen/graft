//! Resolve user TOML config into the JSON spec consumed by Nix.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::config::schema::{
    Container, ContainerConfig, DeployTarget, Filesystem, FilesystemVolume, Network, NetworkMode,
    Runtime, Service, ServiceLifecycle,
};

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
    /// Optional filesystem settings rendered into Quadlet `[Container]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub filesystem: Option<ResolvedFilesystem>,
    /// Optional network settings rendered into Quadlet `[Container]`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub network: Option<ResolvedNetwork>,
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
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
    /// Optional user rendered as Quadlet `User=`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user: Option<String>,
    /// Optional group rendered as Quadlet `Group=`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<String>,
    /// Optional working directory rendered as Quadlet `WorkingDir=`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub working_dir: Option<String>,
    /// Optional environment variables rendered as Quadlet `Environment=`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment: Option<BTreeMap<String, String>>,
    /// Optional environment files rendered as Quadlet `EnvironmentFile=`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub environment_file: Option<Vec<String>>,
}

/// Resolved filesystem settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedFilesystem {
    /// Optional volume mounts rendered as Quadlet `Volume=`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volumes: Option<Vec<ResolvedFilesystemVolume>>,
}

/// Resolved filesystem volume mount.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedFilesystemVolume {
    /// Optional source path or volume name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Required container target path.
    pub target: String,
    /// Optional volume mode/options.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
}

/// Resolved network settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedNetwork {
    /// Optional resolved network namespace.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub namespace: Option<ResolvedNetworkNamespace>,
    /// Optional published ports rendered as Quadlet `PublishPort=`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub publish: Option<Vec<String>>,
}

/// Resolved network namespace with all mode-specific data.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase", tag = "mode")]
pub enum ResolvedNetworkNamespace {
    /// Render Quadlet `Network=none`.
    None,
    /// Render a Quadlet source-unit reference for another container.
    Container {
        /// Referenced Quadlet `.container` source-unit filename.
        unit: String,
    },
}

/// A parsed TOML source supplied explicitly for cross-workload resolution.
#[derive(Debug, Clone, Copy)]
pub struct ConfigSource<'a> {
    unit_name: &'a str,
    origin: &'a str,
    config: &'a ContainerConfig,
}

impl<'a> ConfigSource<'a> {
    /// Create source context from a Quadlet unit stem and parsed configuration.
    #[must_use]
    pub const fn new(unit_name: &'a str, config: &'a ContainerConfig) -> Self {
        Self {
            unit_name,
            origin: unit_name,
            config,
        }
    }

    /// Create source context with a path or other diagnostic origin.
    #[must_use]
    pub const fn with_origin(
        unit_name: &'a str,
        origin: &'a str,
        config: &'a ContainerConfig,
    ) -> Self {
        Self {
            unit_name,
            origin,
            config,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
struct WorkloadKey {
    target: ResolvedDeployTarget,
    name: String,
}

#[derive(Debug)]
struct IndexedWorkload {
    unit_name: String,
    origin: String,
    enabled: bool,
    lifecycle: ServiceLifecycle,
    network_container: Option<String>,
}

#[derive(Debug)]
struct ConfigIndex {
    workloads: BTreeMap<WorkloadKey, IndexedWorkload>,
}

/// Resolved systemd service type.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ResolvedServiceType {
    /// Quadlet notify service for a long-running process.
    Notify,
    /// Foreground systemd oneshot service for a finite process.
    Oneshot,
}

/// Resolved service settings.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedService {
    /// Optional explicit systemd service type.
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub service_type: Option<ResolvedServiceType>,
    /// Optional state retention after a successful finite process exits.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub remain_after_exit: Option<bool>,
    /// Optional systemd restart policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart: Option<String>,
    /// Optional restart delay rendered as systemd `RestartSec=`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub restart_sec: Option<String>,
    /// Optional start timeout rendered as systemd `TimeoutStartSec=`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_start_sec: Option<String>,
    /// Optional stop timeout rendered as systemd `TimeoutStopSec=`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout_stop_sec: Option<String>,
}

/// Resolve a parsed TOML config into the JSON-ready container spec.
///
/// # Errors
///
/// Returns an error when required fields are missing or unsupported values are
/// present.
pub fn resolve(config: &ContainerConfig) -> Result<ResolvedContainer> {
    resolve_internal(config, None)
}

/// Resolve one parsed TOML config using an explicit set of workload sources.
///
/// `config` must be the same parsed instance referenced by one of `sources` so
/// graph validation and final resolution cannot observe different intent.
///
/// # Errors
///
/// Returns an error when source identities or network references are invalid,
/// or when normal container resolution fails.
pub fn resolve_with_context(
    config: &ContainerConfig,
    sources: &[ConfigSource<'_>],
) -> Result<ResolvedContainer> {
    if !requires_config_index(sources) {
        return resolve_internal(config, None);
    }

    if !sources
        .iter()
        .any(|source| std::ptr::eq(source.config, config))
    {
        bail!("current workload is missing from explicit config context");
    }

    let index = ConfigIndex::build(sources)?;
    resolve_internal(config, Some(&index))
}

/// Resolve an explicit set of parsed TOML sources with one shared config index.
///
/// # Errors
///
/// Returns an error when any source, cross-workload reference, or container
/// configuration is invalid.
pub fn resolve_set(sources: &[ConfigSource<'_>]) -> Result<Vec<ResolvedContainer>> {
    let index = requires_config_index(sources)
        .then(|| ConfigIndex::build(sources))
        .transpose()?;

    sources
        .iter()
        .map(|source| {
            resolve_internal(source.config, index.as_ref())
                .with_context(|| format!("failed to resolve config: {}", source.origin))
        })
        .collect()
}

fn requires_config_index(sources: &[ConfigSource<'_>]) -> bool {
    sources.iter().any(|source| {
        source
            .config
            .config
            .as_ref()
            .and_then(|config| config.network.as_ref())
            .is_some_and(|network| network.mode == Some(NetworkMode::Container))
    })
}

fn resolve_internal(
    config: &ContainerConfig,
    context: Option<&ConfigIndex>,
) -> Result<ResolvedContainer> {
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
        filesystem: resolve_filesystem(config)?,
        network: resolve_network(config, context)?,
        service: resolve_service(config)?,
    })
}

fn resolve_container(config: &ContainerConfig) -> Result<Option<ResolvedContainerSettings>> {
    let container = config
        .config
        .as_ref()
        .and_then(|config| config.container.as_ref());
    let hostname = resolve_literal(
        container.and_then(|container| container.hostname.as_deref()),
        |hostname| validate_non_empty_no_control("container hostname", hostname),
    )?;
    let user = resolve_literal(
        container.and_then(|container| container.user.as_deref()),
        |user| validate_non_empty_no_control("container user", user),
    )?;
    let group = resolve_literal(
        container.and_then(|container| container.group.as_deref()),
        |group| validate_non_empty_no_control("container group", group),
    )?;

    if group.is_some() && user.is_none() {
        bail!("container group requires container user");
    }

    let working_dir = resolve_literal(
        container.and_then(|container| container.working_dir.as_deref()),
        |working_dir| validate_non_empty_no_control("container workingDir", working_dir),
    )?;
    let environment = resolve_environment(container)?;
    let environment_file = resolve_environment_file(container)?;

    if hostname.is_none()
        && user.is_none()
        && group.is_none()
        && working_dir.is_none()
        && environment.is_none()
        && environment_file.is_none()
    {
        return Ok(None);
    }

    Ok(Some(ResolvedContainerSettings {
        hostname,
        user,
        group,
        working_dir,
        environment,
        environment_file,
    }))
}

fn resolve_literal<F>(value: Option<&str>, validate: F) -> Result<Option<String>>
where
    F: FnOnce(&str) -> Result<()>,
{
    let Some(value) = value else {
        return Ok(None);
    };

    validate(value)?;

    Ok(Some(value.to_owned()))
}

fn validate_not_empty_or_whitespace(field_name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() {
        bail!("{field_name} cannot be empty");
    }

    Ok(())
}

fn validate_no_control_characters(field_name: &str, value: &str) -> Result<()> {
    if value.chars().any(char::is_control) {
        bail!("{field_name} cannot contain control characters");
    }

    Ok(())
}

fn validate_non_empty_no_control(field_name: &str, value: &str) -> Result<()> {
    validate_not_empty_or_whitespace(field_name, value)?;
    validate_no_control_characters(field_name, value)
}

fn resolve_environment(container: Option<&Container>) -> Result<Option<BTreeMap<String, String>>> {
    let Some(environment) = container.and_then(|container| container.environment.as_ref()) else {
        return Ok(None);
    };

    if environment.is_empty() {
        return Ok(None);
    }

    let mut resolved = BTreeMap::new();

    for (key, value) in environment {
        validate_environment_key(key)?;
        validate_no_control_characters("container environment values", value)?;
        resolved.insert(key.clone(), value.clone());
    }

    Ok(Some(resolved))
}

fn validate_environment_key(key: &str) -> Result<()> {
    validate_non_empty_no_control("container environment keys", key)?;

    if key.chars().any(char::is_whitespace) {
        bail!("container environment keys cannot contain whitespace");
    }

    if key.contains('=') {
        bail!("container environment keys cannot contain equals signs");
    }

    Ok(())
}

fn resolve_environment_file(container: Option<&Container>) -> Result<Option<Vec<String>>> {
    let Some(environment_file) =
        container.and_then(|container| container.environment_file.as_ref())
    else {
        return Ok(None);
    };

    if environment_file.is_empty() {
        return Ok(None);
    }

    for entry in environment_file {
        validate_non_empty_no_control("container environmentFile entries", entry)?;
    }

    Ok(Some(environment_file.clone()))
}

fn resolve_filesystem(config: &ContainerConfig) -> Result<Option<ResolvedFilesystem>> {
    let filesystem = config
        .config
        .as_ref()
        .and_then(|config| config.filesystem.as_ref());
    let volumes = resolve_volumes(filesystem)?;

    if volumes.is_none() {
        return Ok(None);
    }

    Ok(Some(ResolvedFilesystem { volumes }))
}

fn resolve_volumes(
    filesystem: Option<&Filesystem>,
) -> Result<Option<Vec<ResolvedFilesystemVolume>>> {
    let Some(volumes) = filesystem.and_then(|filesystem| filesystem.volumes.as_ref()) else {
        return Ok(None);
    };

    if volumes.is_empty() {
        return Ok(None);
    }

    let mut resolved = Vec::with_capacity(volumes.len());

    for volume in volumes {
        resolved.push(resolve_volume(volume)?);
    }

    Ok(Some(resolved))
}

fn resolve_volume(volume: &FilesystemVolume) -> Result<ResolvedFilesystemVolume> {
    validate_volume_part("target", &volume.target)?;

    if let Some(source) = volume.source.as_deref() {
        validate_volume_part("source", source)?;
    }

    if let Some(mode) = volume.mode.as_deref() {
        validate_volume_part("mode", mode)?;
    }

    if volume.source.is_none() && volume.mode.is_some() {
        bail!("filesystem volume mode requires source");
    }

    Ok(ResolvedFilesystemVolume {
        source: volume.source.clone(),
        target: volume.target.clone(),
        mode: volume.mode.clone(),
    })
}

fn validate_volume_part(name: &str, value: &str) -> Result<()> {
    let field_name = format!("filesystem volume {name}");
    validate_non_empty_no_control(&field_name, value)?;

    if value.contains(':') {
        bail!("filesystem volume {name} cannot contain ':'");
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum NetworkRequest<'a> {
    None,
    Container(&'a str),
}

impl ConfigIndex {
    fn build(sources: &[ConfigSource<'_>]) -> Result<Self> {
        let mut workloads = BTreeMap::new();
        let mut unit_names = BTreeSet::new();

        for source in sources {
            let (key, workload) = index_source(source)
                .with_context(|| format!("invalid config context: {}", source.origin))?;
            if !unit_names.insert((key.target, source.unit_name)) {
                bail!(
                    "duplicate Quadlet source unit '{}' for target '{}' in config context {}",
                    source.unit_name,
                    key.target.as_str(),
                    source.origin
                );
            }
            if workloads.insert(key.clone(), workload).is_some() {
                bail!(
                    "duplicate workload name '{}' for target '{}' in config context {}",
                    key.name,
                    key.target.as_str(),
                    source.origin
                );
            }
        }

        let index = Self { workloads };
        index.validate_references()?;
        index.validate_cycles()?;
        Ok(index)
    }

    fn validate_references(&self) -> Result<()> {
        for (key, workload) in &self.workloads {
            let Some(reference) = workload.network_container.as_ref() else {
                continue;
            };
            let referenced_key = WorkloadKey {
                target: key.target,
                name: reference.clone(),
            };

            if &referenced_key == key {
                bail!(
                    "workload '{}' cannot share its own network namespace in config context {}",
                    key.name,
                    workload.origin
                );
            }

            let Some(referenced) = self.workloads.get(&referenced_key) else {
                if self
                    .workloads
                    .keys()
                    .any(|candidate| candidate.name == *reference)
                {
                    bail!(
                        "network container reference '{}' for workload '{}' has a different deploy target in config context {}",
                        reference,
                        key.name,
                        workload.origin
                    );
                }
                bail!(
                    "network container reference '{}' for workload '{}' was not found in config context {}",
                    reference,
                    key.name,
                    workload.origin
                );
            };

            if !referenced.enabled {
                bail!(
                    "network container reference '{}' for workload '{}' is disabled in config context {}",
                    reference,
                    key.name,
                    workload.origin
                );
            }
            if referenced.lifecycle != ServiceLifecycle::LongRunning {
                bail!(
                    "network container reference '{}' for workload '{}' must use the long-running lifecycle in config context {}",
                    reference,
                    key.name,
                    workload.origin
                );
            }
        }

        Ok(())
    }

    fn validate_cycles(&self) -> Result<()> {
        let mut complete = BTreeSet::new();
        let mut path = Vec::new();

        for key in self.workloads.keys() {
            self.visit(key, &mut complete, &mut path)?;
        }

        Ok(())
    }

    fn visit(
        &self,
        key: &WorkloadKey,
        complete: &mut BTreeSet<WorkloadKey>,
        path: &mut Vec<WorkloadKey>,
    ) -> Result<()> {
        if complete.contains(key) {
            return Ok(());
        }
        if let Some(start) = path.iter().position(|candidate| candidate == key) {
            let mut members = path[start..]
                .iter()
                .map(|candidate| self.cycle_member(candidate))
                .collect::<Result<Vec<_>>>()?;
            members.push(self.cycle_member(key)?);
            bail!(
                "network container reference cycle: {}",
                members.join(" -> ")
            );
        }

        path.push(key.clone());
        if let Some(reference) = self
            .workloads
            .get(key)
            .and_then(|workload| workload.network_container.as_ref())
        {
            let referenced_key = WorkloadKey {
                target: key.target,
                name: reference.clone(),
            };
            self.visit(&referenced_key, complete, path)?;
        }
        path.pop();
        complete.insert(key.clone());
        Ok(())
    }

    fn cycle_member(&self, key: &WorkloadKey) -> Result<String> {
        let workload = self
            .workloads
            .get(key)
            .ok_or_else(|| anyhow::anyhow!("validated cycle member disappeared"))?;
        Ok(format!("{} ({})", key.name, workload.origin))
    }

    fn referenced_unit(&self, config: &ContainerConfig, reference: &str) -> Result<String> {
        let current = workload_key(config)?;
        let referenced = WorkloadKey {
            target: current.target,
            name: reference.to_string(),
        };
        let workload = self
            .workloads
            .get(&referenced)
            .ok_or_else(|| anyhow::anyhow!("validated network reference disappeared"))?;
        Ok(format!("{}.container", workload.unit_name))
    }
}

fn index_source(source: &ConfigSource<'_>) -> Result<(WorkloadKey, IndexedWorkload)> {
    validate_version(source.config)?;
    if !is_safe_container_name(source.unit_name) {
        bail!(
            "Quadlet source unit name contains unsupported characters: {}",
            source.unit_name
        );
    }

    let key = workload_key(source.config)?;
    let network = source
        .config
        .config
        .as_ref()
        .and_then(|config| config.network.as_ref());
    let network_container = match resolve_network_request(network)? {
        Some(NetworkRequest::Container(reference)) => Some(reference.to_string()),
        Some(NetworkRequest::None) | None => None,
    };
    let workload = IndexedWorkload {
        unit_name: source.unit_name.to_string(),
        origin: source.origin.to_string(),
        enabled: source
            .config
            .deploy
            .as_ref()
            .and_then(|deploy| deploy.enable)
            != Some(false),
        lifecycle: effective_lifecycle(source.config),
        network_container,
    };

    Ok((key, workload))
}

impl ResolvedDeployTarget {
    const fn as_str(self) -> &'static str {
        match self {
            Self::System => "system",
            Self::User => "user",
        }
    }
}

fn workload_key(config: &ContainerConfig) -> Result<WorkloadKey> {
    Ok(WorkloadKey {
        target: resolve_deploy_target(
            config
                .deploy
                .as_ref()
                .and_then(|deploy| deploy.target.as_ref()),
        ),
        name: resolve_name(config)?,
    })
}

fn effective_lifecycle(config: &ContainerConfig) -> ServiceLifecycle {
    config
        .config
        .as_ref()
        .and_then(|config| config.service.as_ref())
        .and_then(|service| service.lifecycle)
        .unwrap_or(ServiceLifecycle::LongRunning)
}

fn resolve_network(
    config: &ContainerConfig,
    context: Option<&ConfigIndex>,
) -> Result<Option<ResolvedNetwork>> {
    let network = config
        .config
        .as_ref()
        .and_then(|config| config.network.as_ref());
    let publish = resolve_publish(network)?;
    let namespace = match resolve_network_request(network)? {
        None => None,
        Some(NetworkRequest::None) => Some(ResolvedNetworkNamespace::None),
        Some(NetworkRequest::Container(reference)) => {
            let Some(index) = context else {
                bail!("config.network.mode = \"container\" requires explicit workload context");
            };
            Some(ResolvedNetworkNamespace::Container {
                unit: index.referenced_unit(config, reference)?,
            })
        }
    };

    if namespace.is_none() && publish.is_none() {
        return Ok(None);
    }

    Ok(Some(ResolvedNetwork { namespace, publish }))
}

fn resolve_network_request(network: Option<&Network>) -> Result<Option<NetworkRequest<'_>>> {
    let Some(network) = network else {
        return Ok(None);
    };
    let publish_is_configured = network.publish.is_some();

    match (network.mode, network.container.as_deref()) {
        (None, None) => Ok(None),
        (None | Some(NetworkMode::None), Some(_)) => {
            bail!("config.network.container requires config.network.mode = \"container\"")
        }
        (Some(NetworkMode::None), None) if publish_is_configured => {
            bail!("config.network.publish is incompatible with config.network.mode = \"none\"")
        }
        (Some(NetworkMode::None), None) => Ok(Some(NetworkRequest::None)),
        (Some(NetworkMode::Container), None) => {
            bail!("config.network.mode = \"container\" requires config.network.container")
        }
        (Some(NetworkMode::Container), Some(_)) if publish_is_configured => {
            bail!("config.network.publish is incompatible with config.network.mode = \"container\"")
        }
        (Some(NetworkMode::Container), Some(reference)) => {
            validate_not_empty_or_whitespace("network container reference", reference)?;
            if !is_safe_container_name(reference) {
                bail!("network container reference contains unsupported characters");
            }
            Ok(Some(NetworkRequest::Container(reference)))
        }
    }
}

fn resolve_publish(network: Option<&Network>) -> Result<Option<Vec<String>>> {
    let Some(publish) = network.and_then(|network| network.publish.as_ref()) else {
        return Ok(None);
    };

    if publish.is_empty() {
        return Ok(None);
    }

    for entry in publish {
        validate_non_empty_no_control("network publish entries", entry)?;
    }

    Ok(Some(publish.clone()))
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

    validate_not_empty_or_whitespace("container name", name)?;

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
    let service = config
        .config
        .as_ref()
        .and_then(|config| config.service.as_ref());

    let (service_type, remain_after_exit) = resolve_service_lifecycle(config, service)?;
    let restart = resolve_restart_policy(service)?;
    let restart_sec = resolve_service_timing(
        service.and_then(|service| service.restart_sec.as_deref()),
        "restartSec",
    )?;
    let timeout_start_sec = resolve_service_timing(
        service.and_then(|service| service.timeout_start_sec.as_deref()),
        "timeoutStartSec",
    )?;
    let timeout_stop_sec = resolve_service_timing(
        service.and_then(|service| service.timeout_stop_sec.as_deref()),
        "timeoutStopSec",
    )?;

    let lifecycle = service.and_then(|service| service.lifecycle);
    if matches!(
        lifecycle,
        Some(ServiceLifecycle::Job | ServiceLifecycle::Setup)
    ) {
        if let Some(restart @ ("always" | "on-success" | "on-watchdog")) = restart.as_deref() {
            bail!(
                "config.service.restart = \"{restart}\" is not supported for finite config.service.lifecycle"
            );
        }
    }

    if restart_sec.is_some() && matches!(restart.as_deref(), None | Some("no")) {
        bail!("config.service.restartSec requires config.service.restart other than \"no\"");
    }

    if service_type.is_none()
        && remain_after_exit.is_none()
        && restart.is_none()
        && restart_sec.is_none()
        && timeout_start_sec.is_none()
        && timeout_stop_sec.is_none()
    {
        return Ok(None);
    }

    Ok(Some(ResolvedService {
        service_type,
        remain_after_exit,
        restart,
        restart_sec,
        timeout_start_sec,
        timeout_stop_sec,
    }))
}

fn resolve_service_lifecycle(
    config: &ContainerConfig,
    service: Option<&Service>,
) -> Result<(Option<ResolvedServiceType>, Option<bool>)> {
    let Some(service) = service else {
        return Ok((None, None));
    };

    if service.service_type.is_some() {
        bail!("config.service.type is not supported; use config.service.lifecycle");
    }

    if service.remain_after_exit.is_some() {
        bail!("config.service.remainAfterExit is not supported; use config.service.lifecycle");
    }

    match service.lifecycle {
        None => Ok((None, None)),
        Some(ServiceLifecycle::LongRunning) => Ok((Some(ResolvedServiceType::Notify), None)),
        Some(ServiceLifecycle::Job | ServiceLifecycle::Setup) => {
            let has_explicit_command = config
                .config
                .as_ref()
                .and_then(|config| config.runtime.as_ref())
                .and_then(|runtime| runtime.command.as_ref())
                .is_some();

            if !has_explicit_command {
                bail!(
                    "config.service.lifecycle = \"job\" or \"setup\" requires config.runtime.command"
                );
            }

            Ok((
                Some(ResolvedServiceType::Oneshot),
                Some(service.lifecycle == Some(ServiceLifecycle::Setup)),
            ))
        }
    }
}

fn resolve_restart_policy(service: Option<&Service>) -> Result<Option<String>> {
    resolve_literal(
        service.and_then(|service| service.restart.as_deref()),
        validate_restart_policy,
    )
}

fn validate_restart_policy(restart: &str) -> Result<()> {
    validate_no_control_characters("restart policy", restart)?;

    if matches!(
        restart,
        "no" | "on-success" | "on-failure" | "on-abnormal" | "on-watchdog" | "on-abort" | "always"
    ) {
        Ok(())
    } else {
        bail!("unsupported restart policy: {restart}");
    }
}

fn resolve_service_timing(value: Option<&str>, field_name: &str) -> Result<Option<String>> {
    resolve_literal(value, |value| {
        let field_name = format!("service {field_name}");
        validate_non_empty_no_control(&field_name, value)
    })
}

fn push_unique(packages: &mut Vec<String>, package: &str) {
    if !packages.iter().any(|existing| existing == package) {
        packages.push(package.to_owned());
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, HashMap};

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

    fn filesystem_config(filesystem: Filesystem) -> ContainerConfig {
        ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            name: Some("dev".to_string()),
            config: Some(Config {
                filesystem: Some(filesystem),
                ..Config::default()
            }),
            ..ContainerConfig::default()
        }
    }

    fn network_config(network: Network) -> ContainerConfig {
        ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            name: Some("dev".to_string()),
            config: Some(Config {
                network: Some(network),
                ..Config::default()
            }),
            ..ContainerConfig::default()
        }
    }

    fn contextual_workload(
        name: &str,
        target: ResolvedDeployTarget,
        enable: Option<bool>,
        lifecycle: Option<ServiceLifecycle>,
        network: Option<Network>,
    ) -> ContainerConfig {
        ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            name: Some(name.to_string()),
            deploy: Some(Deploy {
                enable,
                target: Some(match target {
                    ResolvedDeployTarget::System => DeployTarget::System,
                    ResolvedDeployTarget::User => DeployTarget::User,
                }),
            }),
            config: Some(Config {
                network,
                service: lifecycle.map(|lifecycle| Service {
                    lifecycle: Some(lifecycle),
                    ..Service::default()
                }),
                ..Config::default()
            }),
            ..ContainerConfig::default()
        }
    }

    fn container_network(reference: &str) -> Network {
        Network {
            mode: Some(NetworkMode::Container),
            container: Some(reference.to_string()),
            ..Network::default()
        }
    }

    fn service_config(service: Service) -> ContainerConfig {
        ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            name: Some("dev".to_string()),
            config: Some(Config {
                service: Some(service),
                ..Config::default()
            }),
            ..ContainerConfig::default()
        }
    }

    fn finite_service_config(service: Service) -> ContainerConfig {
        ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            name: Some("dev".to_string()),
            config: Some(Config {
                runtime: Some(Runtime {
                    command: Some(vec!["/bin/true".to_string()]),
                    ..Runtime::default()
                }),
                service: Some(service),
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
                user: None,
                group: None,
                working_dir: None,
                environment: None,
                environment_file: None,
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
    fn omitted_user_is_not_rendered_with_hostname() {
        let config = container_config(Container {
            hostname: Some("web.local".to_string()),
            ..Container::default()
        });

        let resolved = resolve(&config).unwrap();
        let container = resolved.container.unwrap();

        assert_eq!(container.hostname, Some("web.local".to_string()));
        assert_eq!(container.user, None);
        assert_eq!(container.group, None);
        assert_eq!(container.working_dir, None);
        assert_eq!(container.environment, None);
        assert_eq!(container.environment_file, None);
    }

    #[test]
    fn explicit_user_is_preserved() {
        let config = container_config(Container {
            user: Some("1000".to_string()),
            ..Container::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.container,
            Some(ResolvedContainerSettings {
                hostname: None,
                user: Some("1000".to_string()),
                group: None,
                working_dir: None,
                environment: None,
                environment_file: None,
            })
        );

        let json = serde_json::to_value(&resolved).unwrap();
        assert_eq!(json["container"]["user"], "1000");
        assert_eq!(json["container"].get("hostname"), None);
        assert_eq!(json["container"].get("group"), None);
        assert_eq!(json["container"].get("workingDir"), None);
    }

    #[test]
    fn hostname_and_user_are_preserved_together() {
        let config = container_config(Container {
            hostname: Some("web.local".to_string()),
            user: Some("1000".to_string()),
            ..Container::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.container,
            Some(ResolvedContainerSettings {
                hostname: Some("web.local".to_string()),
                user: Some("1000".to_string()),
                group: None,
                working_dir: None,
                environment: None,
                environment_file: None,
            })
        );
    }

    #[test]
    fn empty_user_returns_error() {
        let config = container_config(Container {
            user: Some("  ".to_string()),
            ..Container::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn control_character_in_user_returns_error() {
        let config = container_config(Container {
            user: Some("1000\n".to_string()),
            ..Container::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn group_without_user_returns_error() {
        let config = container_config(Container {
            group: Some("1000".to_string()),
            ..Container::default()
        });

        let error = resolve(&config).unwrap_err();

        assert_eq!(error.to_string(), "container group requires container user");
    }

    #[test]
    fn user_and_group_are_preserved_together() {
        let config = container_config(Container {
            user: Some("1000".to_string()),
            group: Some("1000".to_string()),
            ..Container::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.container,
            Some(ResolvedContainerSettings {
                hostname: None,
                user: Some("1000".to_string()),
                group: Some("1000".to_string()),
                working_dir: None,
                environment: None,
                environment_file: None,
            })
        );

        let json = serde_json::to_value(&resolved).unwrap();
        assert_eq!(json["container"]["user"], "1000");
        assert_eq!(json["container"]["group"], "1000");
    }

    #[test]
    fn empty_group_returns_error() {
        let config = container_config(Container {
            group: Some("  ".to_string()),
            ..Container::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn control_character_in_group_returns_error() {
        let config = container_config(Container {
            group: Some("1000\n".to_string()),
            ..Container::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn explicit_working_dir_is_preserved() {
        let config = container_config(Container {
            working_dir: Some("/workspace".to_string()),
            ..Container::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.container,
            Some(ResolvedContainerSettings {
                hostname: None,
                user: None,
                group: None,
                working_dir: Some("/workspace".to_string()),
                environment: None,
                environment_file: None,
            })
        );

        let json = serde_json::to_value(&resolved).unwrap();
        assert_eq!(json["container"]["workingDir"], "/workspace");
        assert_eq!(json["container"].get("hostname"), None);
        assert_eq!(json["container"].get("user"), None);
    }

    #[test]
    fn hostname_user_and_working_dir_are_preserved_together() {
        let config = container_config(Container {
            hostname: Some("web.local".to_string()),
            user: Some("1000".to_string()),
            working_dir: Some("/workspace".to_string()),
            ..Container::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.container,
            Some(ResolvedContainerSettings {
                hostname: Some("web.local".to_string()),
                user: Some("1000".to_string()),
                group: None,
                working_dir: Some("/workspace".to_string()),
                environment: None,
                environment_file: None,
            })
        );
    }

    #[test]
    fn empty_working_dir_returns_error() {
        let config = container_config(Container {
            working_dir: Some("  ".to_string()),
            ..Container::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn control_character_in_working_dir_returns_error() {
        let config = container_config(Container {
            working_dir: Some("/work\nspace".to_string()),
            ..Container::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn empty_environment_is_omitted() {
        let config = container_config(Container {
            environment: Some(HashMap::new()),
            ..Container::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(resolved.container, None);
    }

    #[test]
    fn explicit_environment_is_preserved() {
        let config = container_config(Container {
            environment: Some(HashMap::from([
                ("LOG_LEVEL".to_string(), "debug".to_string()),
                ("EMPTY".to_string(), String::new()),
            ])),
            ..Container::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.container,
            Some(ResolvedContainerSettings {
                hostname: None,
                user: None,
                group: None,
                working_dir: None,
                environment: Some(BTreeMap::from([
                    ("EMPTY".to_string(), String::new()),
                    ("LOG_LEVEL".to_string(), "debug".to_string()),
                ])),
                environment_file: None,
            })
        );

        let json = serde_json::to_value(&resolved).unwrap();
        assert_eq!(json["container"]["environment"]["EMPTY"], "");
        assert_eq!(json["container"]["environment"]["LOG_LEVEL"], "debug");
    }

    #[test]
    fn environment_output_is_sorted() {
        let config = container_config(Container {
            environment: Some(HashMap::from([
                ("Z_LAST".to_string(), "last".to_string()),
                ("A_FIRST".to_string(), "first".to_string()),
                ("MIDDLE".to_string(), "middle".to_string()),
            ])),
            ..Container::default()
        });

        let resolved = resolve(&config).unwrap();
        let environment = resolved.container.unwrap().environment.unwrap();
        let keys = environment.keys().cloned().collect::<Vec<_>>();

        assert_eq!(keys, ["A_FIRST", "MIDDLE", "Z_LAST"]);
    }

    #[test]
    fn empty_environment_key_returns_error() {
        let config = container_config(Container {
            environment: Some(HashMap::from([(String::new(), "value".to_string())])),
            ..Container::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn whitespace_environment_key_returns_error() {
        let config = container_config(Container {
            environment: Some(HashMap::from([(
                "BAD KEY".to_string(),
                "value".to_string(),
            )])),
            ..Container::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn equals_sign_in_environment_key_returns_error() {
        let config = container_config(Container {
            environment: Some(HashMap::from([(
                "BAD=KEY".to_string(),
                "value".to_string(),
            )])),
            ..Container::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn control_character_in_environment_key_returns_error() {
        let config = container_config(Container {
            environment: Some(HashMap::from([(
                "BAD\nKEY".to_string(),
                "value".to_string(),
            )])),
            ..Container::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn whitespace_environment_value_is_preserved() {
        let config = container_config(Container {
            environment: Some(HashMap::from([(
                "GREETING".to_string(),
                "hello world".to_string(),
            )])),
            ..Container::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.container.unwrap().environment.unwrap()["GREETING"],
            "hello world"
        );
    }

    #[test]
    fn quote_sensitive_environment_values_are_preserved() {
        let config = container_config(Container {
            environment: Some(HashMap::from([
                ("EQUALS".to_string(), "a=b".to_string()),
                ("PATHLIKE".to_string(), "C:\\Temp".to_string()),
                ("PERCENT".to_string(), "100%".to_string()),
                ("QUOTED".to_string(), "say \"hi\"".to_string()),
            ])),
            ..Container::default()
        });

        let resolved = resolve(&config).unwrap();
        let json = serde_json::to_value(&resolved).unwrap();

        assert_eq!(json["container"]["environment"]["EQUALS"], "a=b");
        assert_eq!(json["container"]["environment"]["PATHLIKE"], "C:\\Temp");
        assert_eq!(json["container"]["environment"]["PERCENT"], "100%");
        assert_eq!(json["container"]["environment"]["QUOTED"], "say \"hi\"");
    }

    #[test]
    fn control_character_in_environment_value_returns_error() {
        let config = container_config(Container {
            environment: Some(HashMap::from([(
                "BAD".to_string(),
                "line\nbreak".to_string(),
            )])),
            ..Container::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn empty_environment_file_is_omitted() {
        let config = container_config(Container {
            environment_file: Some(Vec::new()),
            ..Container::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(resolved.container, None);
    }

    #[test]
    fn explicit_environment_file_is_preserved() {
        let config = container_config(Container {
            environment_file: Some(vec![
                "/etc/graft/app.env".to_string(),
                "/run/graft/shared.env".to_string(),
            ]),
            ..Container::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.container,
            Some(ResolvedContainerSettings {
                hostname: None,
                user: None,
                group: None,
                working_dir: None,
                environment: None,
                environment_file: Some(vec![
                    "/etc/graft/app.env".to_string(),
                    "/run/graft/shared.env".to_string(),
                ]),
            })
        );

        let json = serde_json::to_value(&resolved).unwrap();
        assert_eq!(
            json["container"]["environmentFile"][0],
            "/etc/graft/app.env"
        );
        assert_eq!(
            json["container"]["environmentFile"][1],
            "/run/graft/shared.env"
        );
        assert_eq!(json["container"].get("environment"), None);
    }

    #[test]
    fn empty_environment_file_entry_returns_error() {
        let config = container_config(Container {
            environment_file: Some(vec![String::new()]),
            ..Container::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn whitespace_environment_file_entry_returns_error() {
        let config = container_config(Container {
            environment_file: Some(vec!["  ".to_string()]),
            ..Container::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn control_character_in_environment_file_entry_returns_error() {
        let config = container_config(Container {
            environment_file: Some(vec!["/etc/graft/app\n.env".to_string()]),
            ..Container::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn volumes_have_no_default() {
        let resolved = resolve(&named_config()).unwrap();
        assert_eq!(resolved.filesystem, None);
    }

    #[test]
    fn empty_volumes_are_omitted() {
        let config = filesystem_config(Filesystem {
            volumes: Some(Vec::new()),
            ..Filesystem::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(resolved.filesystem, None);
    }

    #[test]
    fn target_only_volume_is_preserved() {
        let config = filesystem_config(Filesystem {
            volumes: Some(vec![FilesystemVolume {
                source: None,
                target: "/data".to_string(),
                mode: None,
            }]),
            ..Filesystem::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.filesystem,
            Some(ResolvedFilesystem {
                volumes: Some(vec![ResolvedFilesystemVolume {
                    source: None,
                    target: "/data".to_string(),
                    mode: None,
                }]),
            })
        );

        let json = serde_json::to_value(&resolved).unwrap();
        assert_eq!(json["filesystem"]["volumes"][0]["target"], "/data");
        assert_eq!(json["filesystem"]["volumes"][0].get("source"), None);
        assert_eq!(json["filesystem"]["volumes"][0].get("mode"), None);
    }

    #[test]
    fn source_and_target_volume_is_preserved() {
        let config = filesystem_config(Filesystem {
            volumes: Some(vec![FilesystemVolume {
                source: Some("/host/data".to_string()),
                target: "/data".to_string(),
                mode: None,
            }]),
            ..Filesystem::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.filesystem,
            Some(ResolvedFilesystem {
                volumes: Some(vec![ResolvedFilesystemVolume {
                    source: Some("/host/data".to_string()),
                    target: "/data".to_string(),
                    mode: None,
                }]),
            })
        );
    }

    #[test]
    fn source_target_and_mode_volume_is_preserved() {
        let config = filesystem_config(Filesystem {
            volumes: Some(vec![FilesystemVolume {
                source: Some("/host/config".to_string()),
                target: "/config".to_string(),
                mode: Some("ro".to_string()),
            }]),
            ..Filesystem::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.filesystem,
            Some(ResolvedFilesystem {
                volumes: Some(vec![ResolvedFilesystemVolume {
                    source: Some("/host/config".to_string()),
                    target: "/config".to_string(),
                    mode: Some("ro".to_string()),
                }]),
            })
        );
    }

    #[test]
    fn volume_mode_without_source_returns_error() {
        let config = filesystem_config(Filesystem {
            volumes: Some(vec![FilesystemVolume {
                source: None,
                target: "/data".to_string(),
                mode: Some("ro".to_string()),
            }]),
            ..Filesystem::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn empty_volume_target_returns_error() {
        let config = filesystem_config(Filesystem {
            volumes: Some(vec![FilesystemVolume {
                source: None,
                target: String::new(),
                mode: None,
            }]),
            ..Filesystem::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn whitespace_volume_target_returns_error() {
        let config = filesystem_config(Filesystem {
            volumes: Some(vec![FilesystemVolume {
                source: None,
                target: "  ".to_string(),
                mode: None,
            }]),
            ..Filesystem::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn empty_volume_source_returns_error() {
        let config = filesystem_config(Filesystem {
            volumes: Some(vec![FilesystemVolume {
                source: Some(String::new()),
                target: "/data".to_string(),
                mode: None,
            }]),
            ..Filesystem::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn whitespace_volume_source_returns_error() {
        let config = filesystem_config(Filesystem {
            volumes: Some(vec![FilesystemVolume {
                source: Some("  ".to_string()),
                target: "/data".to_string(),
                mode: None,
            }]),
            ..Filesystem::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn empty_volume_mode_returns_error() {
        let config = filesystem_config(Filesystem {
            volumes: Some(vec![FilesystemVolume {
                source: Some("/host/data".to_string()),
                target: "/data".to_string(),
                mode: Some(String::new()),
            }]),
            ..Filesystem::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn whitespace_volume_mode_returns_error() {
        let config = filesystem_config(Filesystem {
            volumes: Some(vec![FilesystemVolume {
                source: Some("/host/data".to_string()),
                target: "/data".to_string(),
                mode: Some("  ".to_string()),
            }]),
            ..Filesystem::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn control_character_in_volume_target_returns_error() {
        let config = filesystem_config(Filesystem {
            volumes: Some(vec![FilesystemVolume {
                source: None,
                target: "/da\nta".to_string(),
                mode: None,
            }]),
            ..Filesystem::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn control_character_in_volume_source_returns_error() {
        let config = filesystem_config(Filesystem {
            volumes: Some(vec![FilesystemVolume {
                source: Some("/host/da\nta".to_string()),
                target: "/data".to_string(),
                mode: None,
            }]),
            ..Filesystem::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn control_character_in_volume_mode_returns_error() {
        let config = filesystem_config(Filesystem {
            volumes: Some(vec![FilesystemVolume {
                source: Some("/host/data".to_string()),
                target: "/data".to_string(),
                mode: Some("r\no".to_string()),
            }]),
            ..Filesystem::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn colon_in_volume_target_returns_error() {
        let config = filesystem_config(Filesystem {
            volumes: Some(vec![FilesystemVolume {
                source: None,
                target: "/data:logs".to_string(),
                mode: None,
            }]),
            ..Filesystem::default()
        });

        let error = resolve(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "filesystem volume target cannot contain ':'"
        );
    }

    #[test]
    fn colon_in_volume_source_returns_error() {
        let config = filesystem_config(Filesystem {
            volumes: Some(vec![FilesystemVolume {
                source: Some("/host:data".to_string()),
                target: "/data".to_string(),
                mode: None,
            }]),
            ..Filesystem::default()
        });

        let error = resolve(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "filesystem volume source cannot contain ':'"
        );
    }

    #[test]
    fn colon_in_volume_mode_returns_error() {
        let config = filesystem_config(Filesystem {
            volumes: Some(vec![FilesystemVolume {
                source: Some("/host/data".to_string()),
                target: "/data".to_string(),
                mode: Some("ro:z".to_string()),
            }]),
            ..Filesystem::default()
        });

        let error = resolve(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "filesystem volume mode cannot contain ':'"
        );
    }

    #[test]
    fn publish_has_no_default() {
        let resolved = resolve(&named_config()).unwrap();
        assert_eq!(resolved.network, None);
    }

    #[test]
    fn empty_publish_is_omitted() {
        let config = network_config(Network {
            publish: Some(Vec::new()),
            ..Network::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(resolved.network, None);
    }

    #[test]
    fn explicit_publish_is_preserved() {
        let config = network_config(Network {
            publish: Some(vec![
                "127.0.0.1:8080:80".to_string(),
                "8443:443/tcp".to_string(),
            ]),
            ..Network::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.network,
            Some(ResolvedNetwork {
                namespace: None,
                publish: Some(vec![
                    "127.0.0.1:8080:80".to_string(),
                    "8443:443/tcp".to_string(),
                ]),
            })
        );

        let json = serde_json::to_value(&resolved).unwrap();
        assert_eq!(json["network"]["publish"][0], "127.0.0.1:8080:80");
        assert_eq!(json["network"]["publish"][1], "8443:443/tcp");
    }

    #[test]
    fn none_network_namespace_is_resolved() {
        let config = network_config(Network {
            mode: Some(NetworkMode::None),
            ..Network::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.network,
            Some(ResolvedNetwork {
                namespace: Some(ResolvedNetworkNamespace::None),
                publish: None,
            })
        );
        let json = serde_json::to_value(&resolved).unwrap();
        assert_eq!(json["network"]["namespace"]["mode"], "none");
    }

    #[test]
    fn container_network_namespace_resolves_source_unit_from_context() {
        let worker = contextual_workload(
            "worker",
            ResolvedDeployTarget::User,
            None,
            None,
            Some(container_network("database")),
        );
        let database =
            contextual_workload("database", ResolvedDeployTarget::User, None, None, None);
        let sources = [
            ConfigSource::new("worker", &worker),
            ConfigSource::new("database-source", &database),
        ];

        let resolved = resolve_with_context(&worker, &sources).unwrap();

        assert_eq!(
            resolved.network,
            Some(ResolvedNetwork {
                namespace: Some(ResolvedNetworkNamespace::Container {
                    unit: "database-source.container".to_string(),
                }),
                publish: None,
            })
        );
    }

    #[test]
    fn container_network_namespace_requires_context() {
        let config = network_config(container_network("database"));

        let error = resolve(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "config.network.mode = \"container\" requires explicit workload context"
        );
    }

    #[test]
    fn network_container_without_container_mode_returns_error() {
        let config = network_config(Network {
            container: Some("database".to_string()),
            ..Network::default()
        });

        let error = resolve(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "config.network.container requires config.network.mode = \"container\""
        );
    }

    #[test]
    fn container_mode_without_reference_returns_error() {
        let config = network_config(Network {
            mode: Some(NetworkMode::Container),
            ..Network::default()
        });

        let error = resolve(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "config.network.mode = \"container\" requires config.network.container"
        );
    }

    #[test]
    fn non_default_network_modes_reject_published_ports() {
        for network in [
            Network {
                mode: Some(NetworkMode::None),
                publish: Some(Vec::new()),
                ..Network::default()
            },
            Network {
                mode: Some(NetworkMode::None),
                publish: Some(vec!["8080:80".to_string()]),
                ..Network::default()
            },
            Network {
                mode: Some(NetworkMode::Container),
                container: Some("database".to_string()),
                publish: Some(Vec::new()),
                ..Network::default()
            },
            Network {
                mode: Some(NetworkMode::Container),
                container: Some("database".to_string()),
                publish: Some(vec!["8080:80".to_string()]),
                ..Network::default()
            },
        ] {
            let error = resolve(&network_config(network)).unwrap_err();

            assert!(error
                .to_string()
                .starts_with("config.network.publish is incompatible"));
        }
    }

    #[test]
    fn current_workload_missing_from_context_returns_error() {
        let worker = network_config(container_network("database"));
        let database = contextual_workload(
            "database",
            ResolvedDeployTarget::System,
            None,
            None,
            Some(container_network("owner")),
        );
        let owner = contextual_workload("owner", ResolvedDeployTarget::System, None, None, None);
        let sources = [
            ConfigSource::new("database", &database),
            ConfigSource::new("owner", &owner),
        ];

        let error = resolve_with_context(&worker, &sources).unwrap_err();

        assert_eq!(
            error.to_string(),
            "current workload is missing from explicit config context"
        );
    }

    #[test]
    fn same_identity_different_config_is_not_context_membership() {
        let source_worker = contextual_workload(
            "worker",
            ResolvedDeployTarget::System,
            None,
            None,
            Some(container_network("database")),
        );
        let mut caller_worker = source_worker.clone();
        caller_worker
            .config
            .as_mut()
            .unwrap()
            .network
            .as_mut()
            .unwrap()
            .container = Some("different".to_string());
        let database =
            contextual_workload("database", ResolvedDeployTarget::System, None, None, None);
        let sources = [
            ConfigSource::new("worker", &source_worker),
            ConfigSource::new("database", &database),
        ];

        let error = resolve_with_context(&caller_worker, &sources).unwrap_err();

        assert_eq!(
            error.to_string(),
            "current workload is missing from explicit config context"
        );
    }

    #[test]
    fn missing_network_reference_returns_error() {
        let worker = contextual_workload(
            "worker",
            ResolvedDeployTarget::System,
            None,
            None,
            Some(container_network("database")),
        );
        let sources = [ConfigSource::new("worker", &worker)];

        let error = resolve_with_context(&worker, &sources).unwrap_err();

        assert_eq!(
            error.to_string(),
            "network container reference 'database' for workload 'worker' was not found in config context worker"
        );
    }

    #[test]
    fn invalid_network_reference_relationships_return_errors() {
        let cases = vec![
            (
                contextual_workload(
                    "worker",
                    ResolvedDeployTarget::System,
                    None,
                    None,
                    Some(container_network("worker")),
                ),
                None,
                "cannot share its own network namespace",
            ),
            (
                contextual_workload(
                    "worker",
                    ResolvedDeployTarget::System,
                    None,
                    None,
                    Some(container_network("database")),
                ),
                Some(contextual_workload(
                    "database",
                    ResolvedDeployTarget::System,
                    Some(false),
                    None,
                    None,
                )),
                "is disabled",
            ),
            (
                contextual_workload(
                    "worker",
                    ResolvedDeployTarget::System,
                    None,
                    None,
                    Some(container_network("database")),
                ),
                Some(contextual_workload(
                    "database",
                    ResolvedDeployTarget::User,
                    None,
                    None,
                    None,
                )),
                "different deploy target",
            ),
            (
                contextual_workload(
                    "worker",
                    ResolvedDeployTarget::System,
                    None,
                    None,
                    Some(container_network("database")),
                ),
                Some(contextual_workload(
                    "database",
                    ResolvedDeployTarget::System,
                    None,
                    Some(ServiceLifecycle::Job),
                    None,
                )),
                "must use the long-running lifecycle",
            ),
        ];

        for (worker, dependency, expected) in cases {
            let mut sources = vec![ConfigSource::new("worker", &worker)];
            if let Some(dependency) = dependency.as_ref() {
                sources.push(ConfigSource::new("database", dependency));
            }

            let error = resolve_with_context(&worker, &sources).unwrap_err();

            assert!(error.to_string().contains(expected), "{error:#}");
        }
    }

    #[test]
    fn duplicate_workload_name_in_target_returns_error() {
        let first = contextual_workload(
            "database",
            ResolvedDeployTarget::System,
            None,
            None,
            Some(container_network("owner")),
        );
        let second = first.clone();
        let sources = [
            ConfigSource::new("first", &first),
            ConfigSource::new("second", &second),
        ];

        let error = resolve_with_context(&first, &sources).unwrap_err();

        assert_eq!(
            error.to_string(),
            "duplicate workload name 'database' for target 'system' in config context second"
        );
    }

    #[test]
    fn duplicate_source_unit_name_in_target_returns_error() {
        let worker = contextual_workload(
            "worker",
            ResolvedDeployTarget::System,
            None,
            None,
            Some(container_network("database")),
        );
        let database =
            contextual_workload("database", ResolvedDeployTarget::System, None, None, None);
        let sources = [
            ConfigSource::new("shared", &worker),
            ConfigSource::new("shared", &database),
        ];

        let error = resolve_with_context(&worker, &sources).unwrap_err();

        assert_eq!(
            error.to_string(),
            "duplicate Quadlet source unit 'shared' for target 'system' in config context shared"
        );
    }

    #[test]
    fn network_reference_cycle_returns_path() {
        let first = contextual_workload(
            "first",
            ResolvedDeployTarget::System,
            None,
            None,
            Some(container_network("second")),
        );
        let second = contextual_workload(
            "second",
            ResolvedDeployTarget::System,
            None,
            None,
            Some(container_network("first")),
        );
        let sources = [
            ConfigSource::new("first", &first),
            ConfigSource::new("second", &second),
        ];

        let error = resolve_with_context(&first, &sources).unwrap_err();

        assert_eq!(
            error.to_string(),
            "network container reference cycle: first (first) -> second (second) -> first (first)"
        );
    }

    #[test]
    fn unsafe_network_reference_returns_error() {
        let config = network_config(container_network("bad/reference"));

        let error = resolve(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "network container reference contains unsupported characters"
        );
    }

    #[test]
    fn unsafe_source_unit_name_returns_error() {
        let config = network_config(container_network("database"));
        let sources = [ConfigSource::new("bad/unit", &config)];

        let error = resolve_with_context(&config, &sources).unwrap_err();

        assert_eq!(
            format!("{error:#}"),
            "invalid config context: bad/unit: Quadlet source unit name contains unsupported characters: bad/unit"
        );
    }

    #[test]
    fn empty_publish_entry_returns_error() {
        let config = network_config(Network {
            publish: Some(vec![String::new()]),
            ..Network::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn whitespace_publish_entry_returns_error() {
        let config = network_config(Network {
            publish: Some(vec!["  ".to_string()]),
            ..Network::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn control_character_in_publish_entry_returns_error() {
        let config = network_config(Network {
            publish: Some(vec!["8080:\n80".to_string()]),
            ..Network::default()
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
    fn explicit_long_running_lifecycle_resolves_to_notify() {
        let config = service_config(Service {
            lifecycle: Some(ServiceLifecycle::LongRunning),
            ..Service::default()
        });

        let resolved = resolve(&config).unwrap();
        let service = resolved.service.as_ref().unwrap();

        assert_eq!(service.service_type, Some(ResolvedServiceType::Notify));
        assert_eq!(service.remain_after_exit, None);
        let json = serde_json::to_value(&resolved).unwrap();
        assert_eq!(json["service"]["type"], "notify");
        assert_eq!(json["service"].get("remainAfterExit"), None);
    }

    #[test]
    fn finite_lifecycles_resolve_to_oneshot_with_explicit_retention() {
        for (lifecycle, expected_remain_after_exit) in [
            (ServiceLifecycle::Job, false),
            (ServiceLifecycle::Setup, true),
        ] {
            let config = finite_service_config(Service {
                lifecycle: Some(lifecycle),
                ..Service::default()
            });

            let resolved = resolve(&config).unwrap();
            let service = resolved.service.as_ref().unwrap();

            assert_eq!(service.service_type, Some(ResolvedServiceType::Oneshot));
            assert_eq!(service.remain_after_exit, Some(expected_remain_after_exit));
            let json = serde_json::to_value(&resolved).unwrap();
            assert_eq!(json["service"]["type"], "oneshot");
            assert_eq!(
                json["service"]["remainAfterExit"],
                expected_remain_after_exit
            );
        }
    }

    #[test]
    fn finite_lifecycle_without_explicit_command_returns_error() {
        for lifecycle in [ServiceLifecycle::Job, ServiceLifecycle::Setup] {
            let config = service_config(Service {
                lifecycle: Some(lifecycle),
                ..Service::default()
            });

            let error = resolve(&config).unwrap_err();

            assert_eq!(
                error.to_string(),
                "config.service.lifecycle = \"job\" or \"setup\" requires config.runtime.command"
            );
        }
    }

    #[test]
    fn raw_service_type_returns_migration_error() {
        let config = service_config(Service {
            service_type: Some("oneshot".to_string()),
            ..Service::default()
        });

        let error = resolve(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "config.service.type is not supported; use config.service.lifecycle"
        );
    }

    #[test]
    fn raw_remain_after_exit_returns_migration_error() {
        let config = service_config(Service {
            remain_after_exit: Some(false),
            ..Service::default()
        });

        let error = resolve(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "config.service.remainAfterExit is not supported; use config.service.lifecycle"
        );
    }

    #[test]
    fn finite_lifecycle_rejects_incompatible_restart_policies() {
        for restart in ["always", "on-success", "on-watchdog"] {
            let config = finite_service_config(Service {
                lifecycle: Some(ServiceLifecycle::Job),
                restart: Some(restart.to_string()),
                ..Service::default()
            });

            let error = resolve(&config).unwrap_err();

            assert_eq!(
                error.to_string(),
                format!(
                    "config.service.restart = \"{restart}\" is not supported for finite config.service.lifecycle"
                )
            );
        }
    }

    #[test]
    fn finite_lifecycle_accepts_failure_restart_policies() {
        for restart in ["on-failure", "on-abnormal", "on-abort"] {
            let config = finite_service_config(Service {
                lifecycle: Some(ServiceLifecycle::Job),
                restart: Some(restart.to_string()),
                ..Service::default()
            });

            assert!(resolve(&config).is_ok(), "{restart} is accepted");
        }
    }

    #[test]
    fn restart_sec_without_effective_restart_returns_error() {
        for restart in [None, Some("no".to_string())] {
            let config = service_config(Service {
                restart,
                restart_sec: Some("10s".to_string()),
                ..Service::default()
            });

            let error = resolve(&config).unwrap_err();

            assert_eq!(
                error.to_string(),
                "config.service.restartSec requires config.service.restart other than \"no\""
            );
        }
    }

    #[test]
    fn explicit_restart_is_preserved() {
        let config = service_config(Service {
            restart: Some("on-failure".to_string()),
            ..Service::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.service,
            Some(ResolvedService {
                service_type: None,
                remain_after_exit: None,
                restart: Some("on-failure".to_string()),
                restart_sec: None,
                timeout_start_sec: None,
                timeout_stop_sec: None,
            })
        );
    }

    #[test]
    fn explicit_service_timing_is_preserved() {
        let config = service_config(Service {
            restart: Some("on-failure".to_string()),
            restart_sec: Some("10s".to_string()),
            timeout_start_sec: Some("2m".to_string()),
            timeout_stop_sec: Some("30s".to_string()),
            ..Service::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.service,
            Some(ResolvedService {
                service_type: None,
                remain_after_exit: None,
                restart: Some("on-failure".to_string()),
                restart_sec: Some("10s".to_string()),
                timeout_start_sec: Some("2m".to_string()),
                timeout_stop_sec: Some("30s".to_string()),
            })
        );

        let json = serde_json::to_value(&resolved).unwrap();
        assert_eq!(json["service"]["restart"], "on-failure");
        assert_eq!(json["service"]["restartSec"], "10s");
        assert_eq!(json["service"]["timeoutStartSec"], "2m");
        assert_eq!(json["service"]["timeoutStopSec"], "30s");
        assert_eq!(json["service"].get("type"), None);
        assert_eq!(json["service"].get("remainAfterExit"), None);
    }

    #[test]
    fn empty_restart_sec_returns_error() {
        let config = service_config(Service {
            restart_sec: Some(String::new()),
            ..Service::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn whitespace_timeout_start_sec_returns_error() {
        let config = service_config(Service {
            timeout_start_sec: Some("  ".to_string()),
            ..Service::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn control_character_in_timeout_stop_sec_returns_error() {
        let config = service_config(Service {
            timeout_stop_sec: Some("30\ns".to_string()),
            ..Service::default()
        });

        let result = resolve(&config);

        assert!(result.is_err());
    }

    #[test]
    fn all_restart_policies_are_accepted_for_long_running_services() {
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
                        lifecycle: Some(ServiceLifecycle::LongRunning),
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
        assert_eq!(json.get("filesystem"), None);
        assert_eq!(json.get("network"), None);
        assert_eq!(json.get("service"), None);
        assert_eq!(json["deploy"].get("enable"), None);
    }
}
