//! Resolve user TOML config into the JSON spec consumed by Nix.

use std::collections::{BTreeMap, BTreeSet};

use anyhow::{bail, Context, Result};
use serde::Serialize;

use crate::config::schema::{
    Attach, Config, Container, ContainerConfig, Dependency, DependencyLifecycle,
    DependencyOrdering, DependencyRequirement, DependencyTarget, Deploy, DeployActivation,
    DeployTarget, Device, ExternalUnitDependencyTarget, Filesystem, FilesystemBind,
    FilesystemTmpfs, FilesystemTmpfsInput, FilesystemVolume, GraphRefs, Health, Home, Network,
    NetworkMode, PackageOps, Quadlet, Resources, Runtime, Security, Service, ServiceLifecycle,
    Validation, WorkloadDependencyTarget, Workspace,
};

const SUPPORTED_VERSION: u32 = 1;
const GRAFT_PAUSE_PACKAGE: &str = "graft-pause";
const GRAFT_PAUSE_COMMAND: &str = "/bin/graft-pause";
const ROOTFS_STORE_MODE: &str = "rootfs-store";
const SYSTEMD_UNIT_SUFFIXES: [&str; 11] = [
    "service",
    "socket",
    "device",
    "mount",
    "automount",
    "swap",
    "target",
    "path",
    "timer",
    "slice",
    "scope",
];

/// Fully resolved container spec for the NixOS/Home Manager modules.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedContainer {
    /// Container name.
    pub name: String,
    /// Deployment settings.
    pub deploy: ResolvedDeploy,
    /// Optional resolved Quadlet install relationship.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install: Option<ResolvedInstall>,
    /// Optional concrete systemd unit dependencies.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dependencies: Option<ResolvedDependencies>,
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
    /// Concrete security defaults and explicit typed relaxations.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub security: Option<ResolvedSecurity>,
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
    /// Home Manager user-manager container; rootless only for a non-root account.
    User,
}

/// Resolved Quadlet install relationship.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedInstall {
    /// Fixed target that requests the generated service at startup.
    pub wanted_by: ResolvedInstallTarget,
}

/// Fixed systemd target selected for startup activation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ResolvedInstallTarget {
    /// System manager multi-user startup target.
    #[serde(rename = "multi-user.target")]
    MultiUser,
    /// User manager default startup target.
    #[serde(rename = "default.target")]
    Default,
}

/// Concrete dependencies rendered into Quadlet's systemd `[Unit]` section.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedDependencies {
    /// Units required for activation and successful startup.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub requires: Vec<String>,
    /// Units requested without activation-failure coupling.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub wants: Vec<String>,
    /// Units that must finish starting before this workload starts.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub after: Vec<String>,
    /// Units ordered after this workload.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub before: Vec<String>,
    /// Units whose stop and restart operations propagate to this workload.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub part_of: Vec<String>,
    /// Units whose active state this workload is bound to.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub binds_to: Vec<String>,
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
    /// Concrete read-only root filesystem policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read_only: Option<bool>,
    /// Optional ordered typed writable tmpfs mounts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tmpfs: Option<Vec<ResolvedFilesystemTmpfs>>,
    /// Optional ordered non-recursive host bind mounts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub binds: Option<Vec<ResolvedFilesystemBind>>,
    /// Optional ordered Podman-managed volume mounts.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub volumes: Option<Vec<ResolvedFilesystemVolume>>,
    /// Optional qualified CDI references rendered as Quadlet `AddDevice=`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub devices: Option<Vec<ResolvedDevice>>,
}

/// Resolved qualified CDI device reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedDevice {
    /// Validated colon-free CDI qualified name.
    pub source: String,
}

/// Resolved concrete security defaults and explicit typed relaxations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedSecurity {
    /// Concrete drop-all capability baseline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drop_capabilities: Option<Vec<String>>,
    /// Ordered capabilities restored after dropping all defaults.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub add_capabilities: Option<Vec<String>>,
    /// Concrete no-new-privileges policy.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub no_new_privileges: Option<bool>,
}

/// Resolved writable tmpfs mount.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedFilesystemTmpfs {
    /// Required container target path.
    pub target: String,
    /// Optional validated octal mode.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mode: Option<String>,
    /// Optional validated size.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<String>,
}

/// Resolved non-recursive host bind mount.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedFilesystemBind {
    /// Required host source path.
    pub source: String,
    /// Required container target path.
    pub target: String,
    /// Concrete read-only host-access policy.
    pub read_only: bool,
}

/// Resolved Podman-managed volume mount.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ResolvedFilesystemVolume {
    /// Optional named-volume resource; absence requests an anonymous volume.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Required container target path.
    pub target: String,
    /// Concrete read-only policy.
    pub read_only: bool,
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
    dependencies: Vec<DependencyRequest>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum DependencyTargetRequest {
    Workload(String),
    ExternalUnit(String),
}

#[derive(Debug, Clone)]
struct DependencyRequest {
    target: DependencyTargetRequest,
    requirement: Option<DependencyRequirement>,
    ordering: Option<DependencyOrdering>,
    lifecycle: Option<DependencyLifecycle>,
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
        let uses_container_network = source
            .config
            .config
            .as_ref()
            .and_then(|config| config.network.as_ref())
            .is_some_and(|network| network.mode == Some(NetworkMode::Container));
        let uses_workload_dependency =
            source
                .config
                .dependencies
                .as_ref()
                .is_some_and(|dependencies| {
                    dependencies.iter().any(|dependency| {
                        matches!(&dependency.target, DependencyTarget::Workload(_))
                    })
                });

        uses_container_network || uses_workload_dependency
    })
}

macro_rules! reject {
    ($value:expr, $field:literal) => {
        if $value.is_some() {
            bail!(concat!($field, " is configured but not implemented"));
        }
    };
}

fn validate_no_unsupported_intent(source: &ContainerConfig) -> Result<()> {
    // Keep these patterns exhaustive so every parser field must be classified.
    let ContainerConfig {
        version: _,
        name: _,
        parents,
        children,
        dependencies: _,
        deploy,
        validation,
        config,
    } = source;

    if let Some(GraphRefs { add, remove, set }) = parents {
        reject!(add, "parents.add");
        reject!(remove, "parents.remove");
        reject!(set, "parents.set");
    }
    if let Some(GraphRefs { add, remove, set }) = children {
        reject!(add, "children.add");
        reject!(remove, "children.remove");
        reject!(set, "children.set");
    }
    if let Some(deploy) = deploy {
        let Deploy {
            enable: _,
            target: _,
            activation: _,
        } = deploy;
    }
    if let Some(Validation { level }) = validation {
        if level.is_some() {
            bail!(
                "validation.level is configured but not implemented; normal resolution always fails closed"
            );
        }
    }

    let Some(Config {
        runtime,
        container,
        filesystem,
        network,
        networks,
        volumes,
        security,
        resources,
        secrets,
        workspace,
        home,
        attach,
        service,
        quadlet,
    }) = config
    else {
        return Ok(());
    };

    reject!(networks, "config.networks");
    reject!(volumes, "config.volumes");
    reject!(secrets, "config.secrets");

    validate_unsupported_runtime_intent(runtime.as_ref())?;

    validate_unsupported_container_intent(container.as_ref())?;

    validate_unsupported_filesystem_intent(filesystem.as_ref())?;

    validate_unsupported_network_intent(network.as_ref())?;

    validate_unsupported_security_intent(security.as_ref())?;

    validate_unsupported_resources_intent(resources.as_ref())?;

    validate_unsupported_state_intent(workspace.as_ref(), home.as_ref(), attach.as_ref())?;

    validate_unsupported_service_intent(service.as_ref())?;
    validate_unsupported_quadlet_intent(quadlet.as_ref())?;

    Ok(())
}

fn validate_unsupported_runtime_intent(runtime: Option<&Runtime>) -> Result<()> {
    let Some(Runtime {
        mode: _,
        packages: _,
        command: _,
        package_ops,
    }) = runtime
    else {
        return Ok(());
    };

    if let Some(PackageOps {
        add,
        remove,
        replace,
    }) = package_ops
    {
        reject!(add, "config.runtime.packageOps.add");
        reject!(remove, "config.runtime.packageOps.remove");
        reject!(replace, "config.runtime.packageOps.replace");
    }

    Ok(())
}

fn validate_unsupported_container_intent(container: Option<&Container>) -> Result<()> {
    let Some(Container {
        name,
        hostname: _,
        pod,
        entrypoint,
        stop_signal,
        stop_timeout,
        working_dir: _,
        user: _,
        group: _,
        timezone,
        notify,
        run_init,
        annotations,
        environment: _,
        environment_file: _,
        environment_host,
        podman_args,
        global_args,
        ip,
        ip6,
        network_alias,
        expose_host_port,
        uid_map,
        gid_map,
        sub_uid_map,
        sub_gid_map,
        shm_size,
        mask,
        unmask_paths,
        sysctl,
        log_driver,
        health,
    }) = container
    else {
        return Ok(());
    };

    reject!(name, "config.container.name");
    reject!(pod, "config.container.pod");
    reject!(entrypoint, "config.container.entrypoint");
    reject!(stop_signal, "config.container.stopSignal");
    reject!(stop_timeout, "config.container.stopTimeout");
    reject!(timezone, "config.container.timezone");
    reject!(notify, "config.container.notify");
    reject!(run_init, "config.container.runInit");
    reject!(annotations, "config.container.annotations");
    reject!(environment_host, "config.container.environmentHost");
    reject!(podman_args, "config.container.podmanArgs");
    reject!(global_args, "config.container.globalArgs");
    reject!(ip, "config.container.ip");
    reject!(ip6, "config.container.ip6");
    reject!(network_alias, "config.container.networkAlias");
    reject!(expose_host_port, "config.container.exposeHostPort");
    reject!(uid_map, "config.container.uidMap");
    reject!(gid_map, "config.container.gidMap");
    reject!(sub_uid_map, "config.container.subUidMap");
    reject!(sub_gid_map, "config.container.subGidMap");
    reject!(shm_size, "config.container.shmSize");
    reject!(mask, "config.container.mask");
    reject!(unmask_paths, "config.container.unmaskPaths");
    reject!(sysctl, "config.container.sysctl");
    reject!(log_driver, "config.container.logDriver");
    validate_unsupported_health_intent(health.as_ref())
}

fn validate_unsupported_health_intent(health: Option<&Health>) -> Result<()> {
    let Some(Health {
        cmd,
        interval,
        timeout,
        retries,
        start_period,
        on_failure,
        startup_cmd,
        startup_interval,
        startup_retries,
        startup_success,
        startup_timeout,
    }) = health
    else {
        return Ok(());
    };

    reject!(cmd, "config.container.health.cmd");
    reject!(interval, "config.container.health.interval");
    reject!(timeout, "config.container.health.timeout");
    reject!(retries, "config.container.health.retries");
    reject!(start_period, "config.container.health.startPeriod");
    reject!(on_failure, "config.container.health.onFailure");
    reject!(startup_cmd, "config.container.health.startupCmd");
    reject!(startup_interval, "config.container.health.startupInterval");
    reject!(startup_retries, "config.container.health.startupRetries");
    reject!(startup_success, "config.container.health.startupSuccess");
    reject!(startup_timeout, "config.container.health.startupTimeout");

    Ok(())
}

fn validate_unsupported_filesystem_intent(filesystem: Option<&Filesystem>) -> Result<()> {
    let Some(Filesystem {
        read_only: _,
        read_only_tmpfs,
        tmpfs: _,
        mounts,
        binds: _,
        volumes: _,
        devices: _,
    }) = filesystem
    else {
        return Ok(());
    };

    reject!(read_only_tmpfs, "config.filesystem.readOnlyTmpfs");
    reject!(mounts, "config.filesystem.mounts");

    Ok(())
}

fn validate_unsupported_network_intent(network: Option<&Network>) -> Result<()> {
    let Some(Network {
        mode: _,
        container: _,
        publish: _,
        dns,
        dns_option,
        dns_search,
        add_host,
    }) = network
    else {
        return Ok(());
    };

    reject!(dns, "config.network.dns");
    reject!(dns_option, "config.network.dnsOption");
    reject!(dns_search, "config.network.dnsSearch");
    reject!(add_host, "config.network.addHost");

    Ok(())
}

fn validate_unsupported_security_intent(security: Option<&Security>) -> Result<()> {
    let Some(Security {
        drop_capabilities: _,
        add_capabilities: _,
        no_new_privileges: _,
        privileged,
        seccomp_profile,
        security_label_disable,
        security_label_file_type,
        security_label_level,
        security_label_nested,
        security_label_type,
        security_opt,
        userns,
    }) = security
    else {
        return Ok(());
    };

    reject!(privileged, "config.security.privileged");
    reject!(seccomp_profile, "config.security.seccompProfile");
    reject!(
        security_label_disable,
        "config.security.securityLabelDisable"
    );
    reject!(
        security_label_file_type,
        "config.security.securityLabelFileType"
    );
    reject!(security_label_level, "config.security.securityLabelLevel");
    reject!(security_label_nested, "config.security.securityLabelNested");
    reject!(security_label_type, "config.security.securityLabelType");
    reject!(security_opt, "config.security.securityOpt");
    reject!(userns, "config.security.userns");

    Ok(())
}

fn validate_unsupported_resources_intent(resources: Option<&Resources>) -> Result<()> {
    let Some(Resources {
        memory,
        memory_swap,
        cpus,
        cpu_quota,
        pids_limit,
        ulimits,
    }) = resources
    else {
        return Ok(());
    };

    reject!(memory, "config.resources.memory");
    reject!(memory_swap, "config.resources.memorySwap");
    reject!(cpus, "config.resources.cpus");
    reject!(cpu_quota, "config.resources.cpuQuota");
    reject!(pids_limit, "config.resources.pidsLimit");
    reject!(ulimits, "config.resources.ulimits");

    Ok(())
}

fn validate_unsupported_state_intent(
    workspace: Option<&Workspace>,
    home: Option<&Home>,
    attach: Option<&Attach>,
) -> Result<()> {
    if let Some(Workspace {
        mode,
        source,
        target,
        review,
        promote,
        exclude_patterns,
    }) = workspace
    {
        reject!(mode, "config.workspace.mode");
        reject!(source, "config.workspace.source");
        reject!(target, "config.workspace.target");
        reject!(review, "config.workspace.review");
        reject!(promote, "config.workspace.promote");
        reject!(exclude_patterns, "config.workspace.excludePatterns");
    }

    if let Some(Home {
        mode,
        source,
        target,
        review,
        promote,
        ephemeral,
        shadow,
    }) = home
    {
        reject!(mode, "config.home.mode");
        reject!(source, "config.home.source");
        reject!(target, "config.home.target");
        reject!(review, "config.home.review");
        reject!(promote, "config.home.promote");
        reject!(ephemeral, "config.home.ephemeral");
        reject!(shadow, "config.home.shadow");
    }

    if let Some(Attach {
        tmux_session,
        shell,
        start_delay,
    }) = attach
    {
        reject!(tmux_session, "config.attach.tmuxSession");
        reject!(shell, "config.attach.shell");
        reject!(start_delay, "config.attach.startDelay");
    }

    Ok(())
}

fn validate_unsupported_service_intent(service: Option<&Service>) -> Result<()> {
    let Some(Service {
        lifecycle: _,
        service_type,
        restart: _,
        restart_sec: _,
        timeout_start_sec: _,
        timeout_stop_sec: _,
        remain_after_exit,
        restart_if_changed,
    }) = service
    else {
        return Ok(());
    };

    if service_type.is_some() {
        bail!("config.service.type is not supported; use config.service.lifecycle");
    }
    if remain_after_exit.is_some() {
        bail!("config.service.remainAfterExit is not supported; use config.service.lifecycle");
    }
    reject!(restart_if_changed, "config.service.restartIfChanged");

    Ok(())
}

fn validate_unsupported_quadlet_intent(quadlet: Option<&Quadlet>) -> Result<()> {
    let Some(Quadlet {
        container,
        service,
        install,
    }) = quadlet
    else {
        return Ok(());
    };

    reject!(container, "config.quadlet.container");
    reject!(service, "config.quadlet.service");
    if install.is_some() {
        bail!("config.quadlet.install is not supported; use deploy.activation = \"startup\"");
    }

    Ok(())
}

fn resolve_internal(
    config: &ContainerConfig,
    context: Option<&ConfigIndex>,
) -> Result<ResolvedContainer> {
    validate_version(config)?;
    validate_no_unsupported_intent(config)?;

    let name = resolve_name(config)?;
    let runtime = config
        .config
        .as_ref()
        .and_then(|config| config.runtime.as_ref());

    validate_runtime_mode(runtime)?;

    let deploy_target = resolve_deploy_target(
        config
            .deploy
            .as_ref()
            .and_then(|deploy| deploy.target.as_ref()),
    )?;
    let install = resolve_install(config, deploy_target)?;

    Ok(ResolvedContainer {
        name,
        deploy: ResolvedDeploy {
            enable: config.deploy.as_ref().and_then(|deploy| deploy.enable),
            target: deploy_target,
        },
        install,
        dependencies: resolve_dependencies(config, context)?,
        runtime: ResolvedRuntime {
            mode: ROOTFS_STORE_MODE.to_string(),
            packages: resolve_packages(runtime)?,
            command: resolve_command(runtime)?,
        },
        container: resolve_container(config)?,
        filesystem: resolve_filesystem(config)?,
        network: resolve_network(config, context)?,
        security: resolve_security(config)?,
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
    let read_only = Some(
        filesystem
            .and_then(|filesystem| filesystem.read_only)
            .unwrap_or(true),
    );
    let tmpfs = resolve_tmpfs(filesystem)?;
    let binds = resolve_binds(filesystem)?;
    let volumes = resolve_volumes(filesystem)?;
    validate_mount_target_collisions(tmpfs.as_deref(), binds.as_deref(), volumes.as_deref())?;
    let devices = resolve_devices(filesystem)?;

    Ok(Some(ResolvedFilesystem {
        read_only,
        tmpfs,
        binds,
        volumes,
        devices,
    }))
}

fn resolve_tmpfs(filesystem: Option<&Filesystem>) -> Result<Option<Vec<ResolvedFilesystemTmpfs>>> {
    let Some(tmpfs) = filesystem.and_then(|filesystem| filesystem.tmpfs.as_ref()) else {
        return Ok(None);
    };

    if tmpfs.is_empty() {
        return Ok(None);
    }

    let mut resolved = Vec::with_capacity(tmpfs.len());
    for (index, input) in tmpfs.iter().enumerate() {
        let FilesystemTmpfsInput::Typed(tmpfs) = input else {
            bail!(
                "config.filesystem.tmpfs[{index}] uses the legacy path-only form; use [[config.filesystem.tmpfs]] with target"
            );
        };
        resolved.push(resolve_tmpfs_mount(tmpfs, index)?);
    }

    Ok(Some(resolved))
}

fn resolve_tmpfs_mount(tmpfs: &FilesystemTmpfs, index: usize) -> Result<ResolvedFilesystemTmpfs> {
    let target_field = format!("config.filesystem.tmpfs[{index}].target");
    validate_mount_target(&target_field, &tmpfs.target, true)?;

    if let Some(mode) = tmpfs.mode.as_deref() {
        let mode_field = format!("config.filesystem.tmpfs[{index}].mode");
        let valid_digits =
            matches!(mode.len(), 3 | 4) && mode.bytes().all(|byte| matches!(byte, b'0'..=b'7'));
        let parsed = u16::from_str_radix(mode, 8).ok();
        if !valid_digits || !matches!(parsed, Some(value) if value <= 0o1777) {
            bail!("{mode_field} must be a three- or four-digit octal mode no greater than 1777");
        }
    }

    if let Some(size) = tmpfs.size.as_deref() {
        let size_field = format!("config.filesystem.tmpfs[{index}].size");
        let digits = size.strip_suffix(['K', 'M', 'G', 'T']).unwrap_or(size);
        if digits.is_empty()
            || digits.starts_with('0')
            || !digits.bytes().all(|byte| byte.is_ascii_digit())
        {
            bail!("{size_field} must be a positive integer with an optional K, M, G, or T suffix");
        }
    }

    Ok(ResolvedFilesystemTmpfs {
        target: tmpfs.target.clone(),
        mode: tmpfs.mode.clone(),
        size: tmpfs.size.clone(),
    })
}

fn resolve_binds(filesystem: Option<&Filesystem>) -> Result<Option<Vec<ResolvedFilesystemBind>>> {
    let Some(binds) = filesystem.and_then(|filesystem| filesystem.binds.as_ref()) else {
        return Ok(None);
    };

    if binds.is_empty() {
        return Ok(None);
    }

    let mut resolved = Vec::with_capacity(binds.len());
    for (index, bind) in binds.iter().enumerate() {
        resolved.push(resolve_bind(bind, index)?);
    }

    Ok(Some(resolved))
}

fn resolve_bind(bind: &FilesystemBind, index: usize) -> Result<ResolvedFilesystemBind> {
    let source_field = format!("config.filesystem.binds[{index}].source");
    validate_mount_path(&source_field, &bind.source)?;
    if bind.source == "/" {
        bail!("{source_field} cannot expose the host root");
    }
    for protected in ["/proc", "/sys", "/dev", "/run"] {
        if path_is_equal_or_descendant(&bind.source, protected) {
            bail!("{source_field} cannot expose protected host path {protected}");
        }
    }

    let target_field = format!("config.filesystem.binds[{index}].target");
    validate_mount_target(&target_field, &bind.target, false)?;

    Ok(ResolvedFilesystemBind {
        source: bind.source.clone(),
        target: bind.target.clone(),
        read_only: bind.read_only.unwrap_or(true),
    })
}

fn resolve_security(config: &ContainerConfig) -> Result<Option<ResolvedSecurity>> {
    let security = config
        .config
        .as_ref()
        .and_then(|config| config.security.as_ref());

    if let Some(drop_capabilities) = security.and_then(|value| value.drop_capabilities.as_ref()) {
        if drop_capabilities.is_empty() {
            bail!("config.security.dropCapabilities cannot be empty");
        }
        if drop_capabilities.as_slice() != ["all"] {
            bail!(
                "config.security.dropCapabilities must be [\"all\"]; use config.security.addCapabilities for required capabilities"
            );
        }
    }

    let add_capabilities = security.and_then(|value| value.add_capabilities.as_ref());
    if let Some(add_capabilities) = add_capabilities {
        if add_capabilities.is_empty() {
            bail!("config.security.addCapabilities cannot be empty");
        }

        let mut seen = BTreeSet::new();
        for (index, capability) in add_capabilities.iter().enumerate() {
            let field_name = format!("config.security.addCapabilities[{index}]");
            validate_non_empty_no_control(&field_name, capability)?;

            let is_canonical = capability.strip_prefix("CAP_").is_some_and(|name| {
                let mut chars = name.chars();
                chars.next().is_some_and(|first| first.is_ascii_uppercase())
                    && chars.all(|ch| ch.is_ascii_uppercase() || ch.is_ascii_digit() || ch == '_')
            });
            if !is_canonical {
                bail!("{field_name} must be a canonical CAP_* name");
            }
            if !seen.insert(capability) {
                bail!("{field_name} duplicates an earlier capability");
            }
        }
    }

    Ok(Some(ResolvedSecurity {
        drop_capabilities: Some(vec!["all".to_string()]),
        add_capabilities: add_capabilities.cloned(),
        no_new_privileges: Some(
            security
                .and_then(|value| value.no_new_privileges)
                .unwrap_or(true),
        ),
    }))
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
    for (index, volume) in volumes.iter().enumerate() {
        resolved.push(resolve_volume(volume, index)?);
    }

    Ok(Some(resolved))
}

fn resolve_volume(volume: &FilesystemVolume, index: usize) -> Result<ResolvedFilesystemVolume> {
    if let Some(source) = volume.source.as_deref() {
        let migration = if source.starts_with('/') {
            "use [[config.filesystem.binds]] with this absolute source"
        } else if source.starts_with('.') {
            "convert the source to a reviewed absolute host path and use [[config.filesystem.binds]]"
        } else {
            "move the source value to config.filesystem.volumes[].name"
        };
        bail!(
            "config.filesystem.volumes[{index}].source uses the legacy overloaded form; {migration}"
        );
    }
    if volume.mode.is_some() {
        bail!(
            "config.filesystem.volumes[{index}].mode uses legacy raw options; use config.filesystem.volumes[{index}].readOnly"
        );
    }

    if let Some(name) = volume.name.as_deref() {
        validate_volume_name(name, index)?;
    } else if volume.read_only == Some(true) {
        bail!(
            "config.filesystem.volumes[{index}].readOnly cannot be true for an anonymous volume; add a reviewed name or omit readOnly to request writable anonymous storage"
        );
    }

    let target_field = format!("config.filesystem.volumes[{index}].target");
    validate_mount_target(&target_field, &volume.target, false)?;

    Ok(ResolvedFilesystemVolume {
        name: volume.name.clone(),
        target: volume.target.clone(),
        read_only: volume.read_only.unwrap_or(false),
    })
}

fn validate_volume_name(name: &str, index: usize) -> Result<()> {
    let field_name = format!("config.filesystem.volumes[{index}].name");
    let valid = !name.is_empty()
        && name.len() <= 128
        && name.bytes().enumerate().all(|(position, byte)| {
            byte.is_ascii_alphanumeric() || (position > 0 && matches!(byte, b'_' | b'.' | b'-'))
        });
    if !valid {
        bail!("{field_name} must match ^[A-Za-z0-9][A-Za-z0-9_.-]*$ and be at most 128 characters");
    }
    if name.ends_with(".volume") {
        bail!("{field_name} cannot end with '.volume'");
    }
    Ok(())
}

fn validate_mount_target(field_name: &str, target: &str, is_tmpfs: bool) -> Result<()> {
    validate_mount_path(field_name, target)?;
    if target == "/" {
        bail!("{field_name} cannot replace the container rootfs");
    }
    if paths_overlap(target, "/nix/store") {
        bail!("{field_name} cannot overlap Graft-owned path /nix/store");
    }
    for protected in ["/dev", "/proc", "/sys"] {
        if path_is_equal_or_descendant(target, protected) {
            bail!("{field_name} cannot overlap protected container path {protected}");
        }
    }
    if !is_tmpfs {
        for tmpfs_path in ["/run", "/tmp", "/var/tmp"] {
            if path_is_equal_or_descendant(target, tmpfs_path) {
                bail!("{field_name} cannot overlap runtime tmpfs path {tmpfs_path}");
            }
        }
    }
    Ok(())
}

fn validate_mount_path(field_name: &str, path: &str) -> Result<()> {
    validate_non_empty_no_control(field_name, path)?;
    if !path.starts_with('/') {
        bail!("{field_name} must be an absolute path");
    }
    if path.contains(':') {
        bail!("{field_name} cannot contain ':'");
    }
    if path != "/" && path.ends_with('/') {
        bail!("{field_name} cannot end with '/'");
    }
    if path.contains("//") {
        bail!("{field_name} cannot contain repeated '/'");
    }
    if path
        .split('/')
        .any(|component| matches!(component, "." | ".."))
    {
        bail!("{field_name} must be lexically normalized");
    }
    if path.chars().last().is_some_and(char::is_whitespace) {
        bail!("{field_name} cannot end with whitespace");
    }
    if path.ends_with('\\') {
        bail!("{field_name} cannot end with '\\'");
    }
    Ok(())
}

fn validate_mount_target_collisions(
    tmpfs: Option<&[ResolvedFilesystemTmpfs]>,
    binds: Option<&[ResolvedFilesystemBind]>,
    volumes: Option<&[ResolvedFilesystemVolume]>,
) -> Result<()> {
    let mut targets = Vec::new();
    if let Some(tmpfs) = tmpfs {
        targets.extend(tmpfs.iter().enumerate().map(|(index, mount)| {
            (
                format!("config.filesystem.tmpfs[{index}].target"),
                mount.target.as_str(),
            )
        }));
    }
    if let Some(binds) = binds {
        targets.extend(binds.iter().enumerate().map(|(index, mount)| {
            (
                format!("config.filesystem.binds[{index}].target"),
                mount.target.as_str(),
            )
        }));
    }
    if let Some(volumes) = volumes {
        targets.extend(volumes.iter().enumerate().map(|(index, mount)| {
            (
                format!("config.filesystem.volumes[{index}].target"),
                mount.target.as_str(),
            )
        }));
    }

    for (position, (field_name, target)) in targets.iter().enumerate() {
        for (other_field, other_target) in targets.iter().skip(position + 1) {
            if paths_overlap(target, other_target) {
                bail!("{other_field} overlaps {field_name}");
            }
        }
    }
    Ok(())
}

fn paths_overlap(left: &str, right: &str) -> bool {
    path_is_equal_or_descendant(left, right) || path_is_equal_or_descendant(right, left)
}

fn path_is_equal_or_descendant(path: &str, parent: &str) -> bool {
    path == parent
        || path
            .strip_prefix(parent)
            .is_some_and(|suffix| suffix.starts_with('/'))
}

fn resolve_devices(filesystem: Option<&Filesystem>) -> Result<Option<Vec<ResolvedDevice>>> {
    let Some(devices) = filesystem.and_then(|filesystem| filesystem.devices.as_ref()) else {
        return Ok(None);
    };

    if devices.is_empty() {
        return Ok(None);
    }

    let mut sources = BTreeSet::new();
    let mut resolved = Vec::with_capacity(devices.len());

    for (index, device) in devices.iter().enumerate() {
        let resolved_device = resolve_device(device, index)?;
        if !sources.insert(device.source.as_str()) {
            bail!("config.filesystem.devices[{index}].source duplicates an earlier CDI reference");
        }
        resolved.push(resolved_device);
    }

    Ok(Some(resolved))
}

fn resolve_device(device: &Device, index: usize) -> Result<ResolvedDevice> {
    if device.target.is_some() {
        bail!(
            "config.filesystem.devices[{index}].target is configured but CDI target remapping is not supported"
        );
    }
    if device.permissions.is_some() {
        bail!(
            "config.filesystem.devices[{index}].permissions is configured but CDI permissions are not supported"
        );
    }

    let field_name = format!("config.filesystem.devices[{index}].source");
    validate_cdi_qualified_name(&field_name, &device.source)?;

    Ok(ResolvedDevice {
        source: device.source.clone(),
    })
}

fn validate_cdi_qualified_name(field_name: &str, value: &str) -> Result<()> {
    validate_non_empty_no_control(field_name, value)?;

    if value.contains(':') {
        bail!("{field_name} cannot contain ':'");
    }

    let Some((kind, name)) = value.split_once('=') else {
        bail!("{field_name} must be a CDI qualified name in vendor/class=device form");
    };
    let Some((vendor, class)) = kind.split_once('/') else {
        bail!("{field_name} must be a CDI qualified name in vendor/class=device form");
    };

    let valid_kind_component = |component: &str| {
        let mut chars = component.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        component.len() >= 2
            && first.is_ascii_alphabetic()
            && component
                .chars()
                .last()
                .is_some_and(|last| last.is_ascii_alphanumeric())
            && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    };
    let valid_device_name = || {
        let mut chars = name.chars();
        let Some(first) = chars.next() else {
            return false;
        };
        first.is_ascii_alphanumeric()
            && name
                .chars()
                .last()
                .is_some_and(|last| last.is_ascii_alphanumeric())
            && chars.all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '.' | '-' | '_'))
    };

    if value.matches('=').count() != 1
        || kind.matches('/').count() != 1
        || !valid_kind_component(vendor)
        || !valid_kind_component(class)
        || !valid_device_name()
    {
        bail!("{field_name} must be a CDI qualified name in vendor/class=device form");
    }

    Ok(())
}

#[derive(Debug, Clone, Copy)]
enum NetworkRequest<'a> {
    None,
    Container(&'a str),
}

fn dependency_requests(dependencies: Option<&[Dependency]>) -> Result<Vec<DependencyRequest>> {
    let Some(dependencies) = dependencies else {
        return Ok(Vec::new());
    };
    let mut targets = BTreeSet::new();
    let mut requests = Vec::with_capacity(dependencies.len());

    for dependency in dependencies {
        let Dependency {
            target,
            requirement,
            ordering,
            lifecycle,
        } = dependency;
        let target = match target {
            DependencyTarget::Workload(target) => {
                let WorkloadDependencyTarget { workload } = target;
                validate_not_empty_or_whitespace("dependency workload reference", workload)?;
                if !is_safe_container_name(workload) {
                    bail!("dependency workload reference contains unsupported characters");
                }
                DependencyTargetRequest::Workload(workload.clone())
            }
            DependencyTarget::ExternalUnit(target) => {
                let ExternalUnitDependencyTarget { external_unit } = target;
                validate_external_unit_name(external_unit)?;
                DependencyTargetRequest::ExternalUnit(external_unit.clone())
            }
        };
        let target_name = match &target {
            DependencyTargetRequest::Workload(name) => format!("workload:{name}"),
            DependencyTargetRequest::ExternalUnit(name) => format!("externalUnit:{name}"),
        };

        if requirement.is_none() && ordering.is_none() && lifecycle.is_none() {
            bail!("dependency target '{target_name}' must configure at least one relationship");
        }
        if *lifecycle == Some(DependencyLifecycle::Bound) && requirement.is_some() {
            bail!(
                "dependency target '{target_name}' cannot combine requirement with lifecycle = \"bound\"; BindsTo already activates the target"
            );
        }
        if !targets.insert(target.clone()) {
            bail!("duplicate dependency target '{target_name}'");
        }

        requests.push(DependencyRequest {
            target,
            requirement: *requirement,
            ordering: *ordering,
            lifecycle: *lifecycle,
        });
    }

    Ok(requests)
}

fn validate_external_unit_name(name: &str) -> Result<()> {
    validate_non_empty_no_control("dependency externalUnit", name)?;

    if name.len() > 255 {
        bail!("dependency externalUnit cannot exceed 255 characters");
    }
    if !name.is_ascii() || name.chars().any(char::is_whitespace) {
        bail!("dependency externalUnit contains unsupported characters");
    }

    let Some((prefix, suffix)) = name.rsplit_once('.') else {
        bail!("dependency externalUnit must include a supported systemd unit suffix");
    };
    if prefix.is_empty() || !SYSTEMD_UNIT_SUFFIXES.contains(&suffix) {
        bail!("dependency externalUnit must include a supported systemd unit suffix");
    }

    let mut prefix_chars = prefix.chars();
    let first = prefix_chars
        .next()
        .ok_or_else(|| anyhow::anyhow!("dependency externalUnit prefix disappeared"))?;
    if !(first.is_ascii_alphanumeric() || first == '-')
        || prefix_chars
            .any(|ch| !(ch.is_ascii_alphanumeric() || matches!(ch, ':' | '-' | '_' | '.' | '@')))
    {
        bail!("dependency externalUnit contains unsupported characters");
    }

    let at_count = prefix.chars().filter(|ch| *ch == '@').count();
    if at_count > 1 || prefix.starts_with('@') || prefix.ends_with('@') {
        bail!("dependency externalUnit must name a concrete non-template unit");
    }

    Ok(())
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
        index.validate_dependency_references()?;
        index.validate_dependency_cycles()?;
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

    fn validate_dependency_references(&self) -> Result<()> {
        for (key, workload) in &self.workloads {
            for dependency in &workload.dependencies {
                let DependencyTargetRequest::Workload(reference) = &dependency.target else {
                    continue;
                };
                let referenced_key = WorkloadKey {
                    target: key.target,
                    name: reference.clone(),
                };

                if &referenced_key == key {
                    bail!(
                        "workload '{}' cannot depend on itself in config context {}",
                        key.name,
                        workload.origin
                    );
                }

                let Some(referenced) = self.workloads.get(&referenced_key) else {
                    if self
                        .workloads
                        .keys()
                        .any(|candidate| candidate.name.as_str() == reference.as_str())
                    {
                        bail!(
                            "dependency workload reference '{}' for workload '{}' has a different deploy target in config context {}",
                            reference,
                            key.name,
                            workload.origin
                        );
                    }
                    bail!(
                        "dependency workload reference '{}' for workload '{}' was not found in config context {}",
                        reference,
                        key.name,
                        workload.origin
                    );
                };

                if !referenced.enabled {
                    bail!(
                        "dependency workload reference '{}' for workload '{}' is disabled in config context {}",
                        reference,
                        key.name,
                        workload.origin
                    );
                }
                if dependency.lifecycle == Some(DependencyLifecycle::Bound)
                    && referenced.lifecycle == ServiceLifecycle::Job
                {
                    bail!(
                        "dependency workload reference '{}' for workload '{}' cannot bind to the inactive result of a job lifecycle in config context {}",
                        reference,
                        key.name,
                        workload.origin
                    );
                }
            }
        }

        Ok(())
    }

    fn validate_dependency_cycles(&self) -> Result<()> {
        let mut complete = BTreeSet::new();
        let mut path = Vec::new();

        for key in self.workloads.keys() {
            self.visit_dependency(key, &mut complete, &mut path)?;
        }

        Ok(())
    }

    fn visit_dependency(
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
            bail!("workload dependency cycle: {}", members.join(" -> "));
        }

        path.push(key.clone());
        let workload = self
            .workloads
            .get(key)
            .ok_or_else(|| anyhow::anyhow!("validated dependency source disappeared"))?;
        let mut references = workload
            .dependencies
            .iter()
            .filter_map(|dependency| match &dependency.target {
                DependencyTargetRequest::Workload(reference) => Some(reference),
                DependencyTargetRequest::ExternalUnit(_) => None,
            })
            .collect::<BTreeSet<_>>();
        if let Some(reference) = workload.network_container.as_ref() {
            references.insert(reference);
        }
        for reference in references {
            let referenced_key = WorkloadKey {
                target: key.target,
                name: reference.clone(),
            };
            self.visit_dependency(&referenced_key, complete, path)?;
        }
        path.pop();
        complete.insert(key.clone());
        Ok(())
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
            .ok_or_else(|| anyhow::anyhow!("validated reference disappeared"))?;
        Ok(format!("{}.container", workload.unit_name))
    }
}

fn index_source(source: &ConfigSource<'_>) -> Result<(WorkloadKey, IndexedWorkload)> {
    validate_version(source.config)?;
    validate_no_unsupported_intent(source.config)?;
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
        dependencies: dependency_requests(source.config.dependencies.as_deref())?,
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
        )?,
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

fn resolve_dependencies(
    config: &ContainerConfig,
    context: Option<&ConfigIndex>,
) -> Result<Option<ResolvedDependencies>> {
    let requests = dependency_requests(config.dependencies.as_deref())?;
    if requests.is_empty() {
        return Ok(None);
    }

    let mut identities = BTreeSet::new();
    let mut requires = BTreeSet::new();
    let mut wants = BTreeSet::new();
    let mut after = BTreeSet::new();
    let mut before = BTreeSet::new();
    let mut part_of = BTreeSet::new();
    let mut binds_to = BTreeSet::new();

    for request in requests {
        let unit = match &request.target {
            DependencyTargetRequest::Workload(reference) => {
                let Some(index) = context else {
                    bail!("workload dependencies require explicit workload context");
                };
                index.referenced_unit(config, reference)?
            }
            DependencyTargetRequest::ExternalUnit(unit) => unit.clone(),
        };
        let identity = unit
            .strip_suffix(".container")
            .map_or_else(|| unit.clone(), |stem| format!("{stem}.service"));
        if !identities.insert(identity.clone()) {
            bail!("multiple dependency targets resolve to unit '{identity}'");
        }

        match request.requirement {
            Some(DependencyRequirement::Required) => {
                requires.insert(unit.clone());
            }
            Some(DependencyRequirement::Optional) => {
                wants.insert(unit.clone());
            }
            None => {}
        }
        match request.ordering {
            Some(DependencyOrdering::After) => {
                after.insert(unit.clone());
            }
            Some(DependencyOrdering::Before) => {
                before.insert(unit.clone());
            }
            None => {}
        }
        match request.lifecycle {
            Some(DependencyLifecycle::PartOf) => {
                part_of.insert(unit.clone());
            }
            Some(DependencyLifecycle::Bound) => {
                binds_to.insert(unit);
            }
            None => {}
        }
    }

    Ok(Some(ResolvedDependencies {
        requires: requires.into_iter().collect(),
        wants: wants.into_iter().collect(),
        after: after.into_iter().collect(),
        before: before.into_iter().collect(),
        part_of: part_of.into_iter().collect(),
        binds_to: binds_to.into_iter().collect(),
    }))
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

fn resolve_deploy_target(target: Option<&DeployTarget>) -> Result<ResolvedDeployTarget> {
    match target {
        Some(DeployTarget::User) => Ok(ResolvedDeployTarget::User),
        Some(DeployTarget::System) => Ok(ResolvedDeployTarget::System),
        None => bail!(
            "deploy.target is required; set deploy.target = \"user\" or deploy.target = \"system\""
        ),
    }
}

fn resolve_install(
    config: &ContainerConfig,
    deploy_target: ResolvedDeployTarget,
) -> Result<Option<ResolvedInstall>> {
    let raw_install = config
        .config
        .as_ref()
        .and_then(|config| config.quadlet.as_ref())
        .and_then(|quadlet| quadlet.install.as_ref());
    if raw_install.is_some() {
        bail!("config.quadlet.install is not supported; use deploy.activation = \"startup\"");
    }

    let activation = config.deploy.as_ref().and_then(|deploy| deploy.activation);
    let Some(DeployActivation::Startup) = activation else {
        return Ok(None);
    };

    let wanted_by = match deploy_target {
        ResolvedDeployTarget::System => ResolvedInstallTarget::MultiUser,
        ResolvedDeployTarget::User => ResolvedInstallTarget::Default,
    };

    Ok(Some(ResolvedInstall { wanted_by }))
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

    use crate::config::schema::{Config, Deploy, Quadlet, Service};

    use super::*;

    fn named_config() -> ContainerConfig {
        ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            name: Some("dev".to_string()),
            deploy: Some(Deploy {
                target: Some(DeployTarget::System),
                ..Deploy::default()
            }),
            ..ContainerConfig::default()
        }
    }

    fn runtime_config(runtime: Runtime) -> ContainerConfig {
        ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            name: Some("dev".to_string()),
            deploy: Some(Deploy {
                target: Some(DeployTarget::System),
                ..Deploy::default()
            }),
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
            deploy: Some(Deploy {
                target: Some(DeployTarget::System),
                ..Deploy::default()
            }),
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
            deploy: Some(Deploy {
                target: Some(DeployTarget::System),
                ..Deploy::default()
            }),
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
            deploy: Some(Deploy {
                target: Some(DeployTarget::System),
                ..Deploy::default()
            }),
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
                ..Deploy::default()
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

    fn workload_dependency(
        workload: &str,
        requirement: Option<DependencyRequirement>,
        ordering: Option<DependencyOrdering>,
        lifecycle: Option<DependencyLifecycle>,
    ) -> Dependency {
        Dependency {
            target: DependencyTarget::Workload(WorkloadDependencyTarget {
                workload: workload.to_string(),
            }),
            requirement,
            ordering,
            lifecycle,
        }
    }

    fn external_unit_dependency(
        unit: &str,
        requirement: Option<DependencyRequirement>,
        ordering: Option<DependencyOrdering>,
        lifecycle: Option<DependencyLifecycle>,
    ) -> Dependency {
        Dependency {
            target: DependencyTarget::ExternalUnit(ExternalUnitDependencyTarget {
                external_unit: unit.to_string(),
            }),
            requirement,
            ordering,
            lifecycle,
        }
    }

    fn service_config(service: Service) -> ContainerConfig {
        ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            name: Some("dev".to_string()),
            deploy: Some(Deploy {
                target: Some(DeployTarget::System),
                ..Deploy::default()
            }),
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
            deploy: Some(Deploy {
                target: Some(DeployTarget::System),
                ..Deploy::default()
            }),
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

    fn config_with_toml(snippet: &str) -> ContainerConfig {
        toml::from_str(&format!(
            "version = 1\nname = \"dev\"\n\n[deploy]\ntarget = \"system\"\n\n{snippet}\n"
        ))
        .unwrap()
    }

    const UNSUPPORTED_FIELD_CASES: &[(&str, &str)] = &[
        ("[parents]\nadd = [\"base\"]", "parents.add"),
        ("[parents]\nremove = [\"base\"]", "parents.remove"),
        ("[parents]\nset = [\"base\"]", "parents.set"),
        ("[children]\nadd = [\"worker\"]", "children.add"),
        ("[children]\nremove = [\"worker\"]", "children.remove"),
        ("[children]\nset = [\"worker\"]", "children.set"),
        (
            "[config.runtime.packageOps]\nadd = [\"curl\"]",
            "config.runtime.packageOps.add",
        ),
        (
            "[config.runtime.packageOps]\nremove = [\"curl\"]",
            "config.runtime.packageOps.remove",
        ),
        (
            "[[config.runtime.packageOps.replace]]\nname = \"curl\"\nwith = \"wget\"",
            "config.runtime.packageOps.replace",
        ),
        (
            "[config]\nnetworks = [{ name = \"private\" }]",
            "config.networks",
        ),
        (
            "[config]\nvolumes = [{ name = \"data\" }]",
            "config.volumes",
        ),
        (
            "[config]\nsecrets = [{ name = \"token\" }]",
            "config.secrets",
        ),
        (
            "[config.container]\nname = \"runtime-name\"",
            "config.container.name",
        ),
        (
            "[config.container]\npod = \"app.pod\"",
            "config.container.pod",
        ),
        (
            "[config.container]\nentrypoint = [\"/bin/sh\"]",
            "config.container.entrypoint",
        ),
        (
            "[config.container]\nstopSignal = \"SIGTERM\"",
            "config.container.stopSignal",
        ),
        (
            "[config.container]\nstopTimeout = 0",
            "config.container.stopTimeout",
        ),
        (
            "[config.container]\ntimezone = \"UTC\"",
            "config.container.timezone",
        ),
        (
            "[config.container]\nnotify = \"healthy\"",
            "config.container.notify",
        ),
        (
            "[config.container]\nrunInit = false",
            "config.container.runInit",
        ),
        (
            "[config.container]\nannotations = { role = \"worker\" }",
            "config.container.annotations",
        ),
        (
            "[config.container]\nenvironmentHost = false",
            "config.container.environmentHost",
        ),
        (
            "[config.container]\npodmanArgs = [\"--privileged\"]",
            "config.container.podmanArgs",
        ),
        (
            "[config.container]\nglobalArgs = [\"--log-level=debug\"]",
            "config.container.globalArgs",
        ),
        (
            "[config.container]\nip = \"10.0.0.2\"",
            "config.container.ip",
        ),
        (
            "[config.container]\nip6 = \"fd00::2\"",
            "config.container.ip6",
        ),
        (
            "[config.container]\nnetworkAlias = [\"api\"]",
            "config.container.networkAlias",
        ),
        (
            "[config.container]\nexposeHostPort = [\"8080\"]",
            "config.container.exposeHostPort",
        ),
        (
            "[config.container]\nuidMap = [\"0:100000:65536\"]",
            "config.container.uidMap",
        ),
        (
            "[config.container]\ngidMap = [\"0:100000:65536\"]",
            "config.container.gidMap",
        ),
        (
            "[config.container]\nsubUidMap = \"@user\"",
            "config.container.subUidMap",
        ),
        (
            "[config.container]\nsubGidMap = \"@user\"",
            "config.container.subGidMap",
        ),
        (
            "[config.container]\nshmSize = \"64m\"",
            "config.container.shmSize",
        ),
        (
            "[config.container]\nmask = [\"/proc/kcore\"]",
            "config.container.mask",
        ),
        (
            "[config.container]\nunmaskPaths = [\"/proc/acpi\"]",
            "config.container.unmaskPaths",
        ),
        (
            "[config.container]\nsysctl = [\"net.ipv4.ip_forward=1\"]",
            "config.container.sysctl",
        ),
        (
            "[config.container]\nlogDriver = \"journald\"",
            "config.container.logDriver",
        ),
        (
            "[config.container.health]\ncmd = \"/bin/true\"",
            "config.container.health.cmd",
        ),
        (
            "[config.container.health]\ninterval = \"30s\"",
            "config.container.health.interval",
        ),
        (
            "[config.container.health]\ntimeout = \"10s\"",
            "config.container.health.timeout",
        ),
        (
            "[config.container.health]\nretries = 0",
            "config.container.health.retries",
        ),
        (
            "[config.container.health]\nstartPeriod = \"5s\"",
            "config.container.health.startPeriod",
        ),
        (
            "[config.container.health]\nonFailure = \"kill\"",
            "config.container.health.onFailure",
        ),
        (
            "[config.container.health]\nstartupCmd = \"/bin/true\"",
            "config.container.health.startupCmd",
        ),
        (
            "[config.container.health]\nstartupInterval = \"5s\"",
            "config.container.health.startupInterval",
        ),
        (
            "[config.container.health]\nstartupRetries = 0",
            "config.container.health.startupRetries",
        ),
        (
            "[config.container.health]\nstartupSuccess = 0",
            "config.container.health.startupSuccess",
        ),
        (
            "[config.container.health]\nstartupTimeout = \"5s\"",
            "config.container.health.startupTimeout",
        ),
        (
            "[config.filesystem]\nreadOnlyTmpfs = false",
            "config.filesystem.readOnlyTmpfs",
        ),
        (
            "[config.filesystem]\nmounts = [\"type=bind,src=/tmp,dst=/data\"]",
            "config.filesystem.mounts",
        ),
        (
            "[config.network]\ndns = [\"1.1.1.1\"]",
            "config.network.dns",
        ),
        (
            "[config.network]\ndnsOption = [\"ndots:1\"]",
            "config.network.dnsOption",
        ),
        (
            "[config.network]\ndnsSearch = [\"example.test\"]",
            "config.network.dnsSearch",
        ),
        (
            "[config.network]\naddHost = [\"api:127.0.0.1\"]",
            "config.network.addHost",
        ),
        (
            "[config.security]\nprivileged = false",
            "config.security.privileged",
        ),
        (
            "[config.security]\nseccompProfile = \"default.json\"",
            "config.security.seccompProfile",
        ),
        (
            "[config.security]\nsecurityLabelDisable = false",
            "config.security.securityLabelDisable",
        ),
        (
            "[config.security]\nsecurityLabelFileType = \"container_file_t\"",
            "config.security.securityLabelFileType",
        ),
        (
            "[config.security]\nsecurityLabelLevel = \"s0:c1,c2\"",
            "config.security.securityLabelLevel",
        ),
        (
            "[config.security]\nsecurityLabelNested = false",
            "config.security.securityLabelNested",
        ),
        (
            "[config.security]\nsecurityLabelType = \"container_t\"",
            "config.security.securityLabelType",
        ),
        (
            "[config.security]\nsecurityOpt = [\"no-new-privileges\"]",
            "config.security.securityOpt",
        ),
        (
            "[config.security]\nuserns = \"keep-id\"",
            "config.security.userns",
        ),
        (
            "[config.resources]\nmemory = \"512m\"",
            "config.resources.memory",
        ),
        (
            "[config.resources]\nmemorySwap = \"1g\"",
            "config.resources.memorySwap",
        ),
        (
            "[config.resources]\ncpus = \"0.5\"",
            "config.resources.cpus",
        ),
        (
            "[config.resources]\ncpuQuota = \"50%\"",
            "config.resources.cpuQuota",
        ),
        (
            "[config.resources]\npidsLimit = 0",
            "config.resources.pidsLimit",
        ),
        (
            "[config.resources]\nulimits = [\"nofile=1024:1024\"]",
            "config.resources.ulimits",
        ),
        (
            "[config.workspace]\nmode = \"copy\"",
            "config.workspace.mode",
        ),
        (
            "[config.workspace]\nsource = \".\"",
            "config.workspace.source",
        ),
        (
            "[config.workspace]\ntarget = \"/workspace\"",
            "config.workspace.target",
        ),
        (
            "[config.workspace]\nreview = \"diff\"",
            "config.workspace.review",
        ),
        (
            "[config.workspace]\npromote = \"off\"",
            "config.workspace.promote",
        ),
        (
            "[config.workspace]\nexcludePatterns = [\".git\"]",
            "config.workspace.excludePatterns",
        ),
        ("[config.home]\nmode = \"ephemeral\"", "config.home.mode"),
        ("[config.home]\nsource = \"~/.home\"", "config.home.source"),
        (
            "[config.home]\ntarget = \"/home/user\"",
            "config.home.target",
        ),
        ("[config.home]\nreview = \"diff\"", "config.home.review"),
        ("[config.home]\npromote = \"never\"", "config.home.promote"),
        ("[config.home]\nephemeral = false", "config.home.ephemeral"),
        (
            "[config.home]\nshadow = [{ container = \"/cache\", host = \"~/.cache\" }]",
            "config.home.shadow",
        ),
        (
            "[config.attach]\ntmuxSession = \"main\"",
            "config.attach.tmuxSession",
        ),
        (
            "[config.attach]\nshell = \"/bin/bash\"",
            "config.attach.shell",
        ),
        (
            "[config.attach]\nstartDelay = \"500ms\"",
            "config.attach.startDelay",
        ),
        (
            "[config.service]\nrestartIfChanged = false",
            "config.service.restartIfChanged",
        ),
        (
            "[config.quadlet.container]\nPodmanArgs = [\"--privileged\"]",
            "config.quadlet.container",
        ),
        (
            "[config.quadlet.service]\nEnvironment = [\"MODE=unsafe\"]",
            "config.quadlet.service",
        ),
    ];

    #[test]
    fn configured_unsupported_fields_return_field_specific_errors() {
        for &(snippet, field) in UNSUPPORTED_FIELD_CASES {
            let config = config_with_toml(snippet);
            let error = resolve(&config).unwrap_err();

            assert_eq!(
                error.to_string(),
                format!("{field} is configured but not implemented"),
                "unexpected diagnostic for {field}"
            );
        }
    }

    #[test]
    fn validation_level_cannot_disable_fail_closed_resolution() {
        for level in ["off", "warn", "strict"] {
            let config = config_with_toml(&format!(
                "[validation]\nlevel = \"{level}\"\n\n[config.security]\nprivileged = true"
            ));
            let error = resolve(&config).unwrap_err();

            assert_eq!(
                error.to_string(),
                "validation.level is configured but not implemented; normal resolution always fails closed"
            );
        }
    }

    #[test]
    fn explicit_empty_unsupported_leaf_values_return_errors() {
        let cases = [
            ("[parents]\nadd = []", "parents.add"),
            ("[config]\nnetworks = []", "config.networks"),
            (
                "[config.container]\npodmanArgs = []",
                "config.container.podmanArgs",
            ),
            (
                "[config.container]\nannotations = {}",
                "config.container.annotations",
            ),
            ("[config.quadlet.container]", "config.quadlet.container"),
        ];

        for (snippet, field) in cases {
            let config = config_with_toml(snippet);
            let error = resolve(&config).unwrap_err();

            assert_eq!(
                error.to_string(),
                format!("{field} is configured but not implemented"),
                "unexpected diagnostic for {field}"
            );
        }
    }

    #[test]
    fn empty_reserved_sections_do_not_create_false_positives() {
        let config = config_with_toml(
            "[parents]\n\
             [children]\n\
             [validation]\n\
             [config]\n\
             [config.runtime]\n\
             [config.runtime.packageOps]\n\
             [config.container]\n\
             [config.container.health]\n\
             [config.filesystem]\n\
             [config.network]\n\
             [config.security]\n\
             [config.resources]\n\
             [config.workspace]\n\
             [config.home]\n\
             [config.attach]\n\
             [config.service]\n\
             [config.quadlet]",
        );

        let resolved = resolve(&config).unwrap();

        assert_eq!(resolved.name, "dev");
        assert_eq!(resolved.runtime.command, [GRAFT_PAUSE_COMMAND]);
    }

    #[test]
    fn unsupported_context_intent_returns_field_and_origin() {
        let worker = contextual_workload(
            "worker",
            ResolvedDeployTarget::System,
            None,
            None,
            Some(container_network("database")),
        );
        let mut database =
            contextual_workload("database", ResolvedDeployTarget::System, None, None, None);
        database.config.as_mut().unwrap().resources = Some(Resources {
            memory: Some("512m".to_string()),
            ..Resources::default()
        });
        let sources = [
            ConfigSource::with_origin("worker", "worker.toml", &worker),
            ConfigSource::with_origin("database", "database.toml", &database),
        ];

        let error = resolve_with_context(&worker, &sources).unwrap_err();

        assert_eq!(
            format!("{error:#}"),
            "invalid config context: database.toml: config.resources.memory is configured but not implemented"
        );
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
        assert_eq!(resolved.filesystem.unwrap().volumes, None);
    }

    #[test]
    fn empty_volumes_are_omitted() {
        let config = filesystem_config(Filesystem {
            volumes: Some(Vec::new()),
            ..Filesystem::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(resolved.filesystem.unwrap().volumes, None);
    }

    #[test]
    fn typed_tmpfs_entries_preserve_order_and_options() {
        let config = filesystem_config(Filesystem {
            tmpfs: Some(vec![
                FilesystemTmpfsInput::Typed(FilesystemTmpfs {
                    target: "/run/graft".to_string(),
                    mode: Some("0750".to_string()),
                    size: Some("64M".to_string()),
                }),
                FilesystemTmpfsInput::Typed(FilesystemTmpfs {
                    target: "/tmp/graft".to_string(),
                    mode: None,
                    size: None,
                }),
            ]),
            ..Filesystem::default()
        });

        let resolved = resolve(&config).unwrap();
        let tmpfs = resolved.filesystem.unwrap().tmpfs.unwrap();

        assert_eq!(tmpfs[0].target, "/run/graft");
        assert_eq!(tmpfs[0].mode.as_deref(), Some("0750"));
        assert_eq!(tmpfs[0].size.as_deref(), Some("64M"));
        assert_eq!(tmpfs[1].target, "/tmp/graft");
    }

    #[test]
    fn legacy_path_only_tmpfs_returns_migration_error() {
        let config = filesystem_config(Filesystem {
            tmpfs: Some(vec![FilesystemTmpfsInput::Legacy("/tmp".to_string())]),
            ..Filesystem::default()
        });

        let error = resolve(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "config.filesystem.tmpfs[0] uses the legacy path-only form; use [[config.filesystem.tmpfs]] with target"
        );
    }

    #[test]
    fn empty_tmpfs_is_omitted() {
        let config = filesystem_config(Filesystem {
            tmpfs: Some(Vec::new()),
            ..Filesystem::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(resolved.filesystem.unwrap().tmpfs, None);
    }

    #[test]
    fn invalid_mount_paths_return_field_specific_errors() {
        let cases = [
            ("", "cannot be empty"),
            ("relative", "must be an absolute path"),
            ("/data:rw", "cannot contain ':'"),
            ("/data/", "cannot end with '/'"),
            ("/data//logs", "cannot contain repeated '/'"),
            ("/data/../logs", "must be lexically normalized"),
            ("/data ", "cannot end with whitespace"),
            ("/data\\", "cannot end with '\\'"),
            ("/da\nta", "cannot contain control characters"),
        ];

        for (path, expected) in cases {
            let config = filesystem_config(Filesystem {
                tmpfs: Some(vec![FilesystemTmpfsInput::Typed(FilesystemTmpfs {
                    target: path.to_string(),
                    mode: None,
                    size: None,
                })]),
                ..Filesystem::default()
            });

            let error = resolve(&config).unwrap_err();

            assert!(error.to_string().contains(expected), "{path}: {error}");
        }
    }

    #[test]
    fn invalid_tmpfs_modes_and_sizes_return_indexed_errors() {
        let cases = [
            (Some("22"), None, "mode"),
            (Some("2777"), None, "mode"),
            (Some("08x0"), None, "mode"),
            (None, Some("0"), "size"),
            (None, Some("01M"), "size"),
            (None, Some("1MiB"), "size"),
        ];

        for (mode, size, expected_field) in cases {
            let config = filesystem_config(Filesystem {
                tmpfs: Some(vec![FilesystemTmpfsInput::Typed(FilesystemTmpfs {
                    target: "/cache".to_string(),
                    mode: mode.map(str::to_string),
                    size: size.map(str::to_string),
                })]),
                ..Filesystem::default()
            });

            let error = resolve(&config).unwrap_err();

            assert!(error
                .to_string()
                .contains(&format!("config.filesystem.tmpfs[0].{expected_field}")));
        }
    }

    #[test]
    fn bind_defaults_read_only_and_explicit_false_is_preserved() {
        let config = filesystem_config(Filesystem {
            binds: Some(vec![
                FilesystemBind {
                    source: "/srv/config".to_string(),
                    target: "/config".to_string(),
                    read_only: None,
                },
                FilesystemBind {
                    source: "/srv/work".to_string(),
                    target: "/workspace".to_string(),
                    read_only: Some(false),
                },
            ]),
            ..Filesystem::default()
        });

        let resolved = resolve(&config).unwrap();
        let binds = resolved.filesystem.unwrap().binds.unwrap();

        assert!(binds[0].read_only);
        assert!(!binds[1].read_only);
    }

    #[test]
    fn protected_bind_sources_return_indexed_errors() {
        for source in [
            "/",
            "/proc",
            "/proc/sys",
            "/sys",
            "/dev/kvm",
            "/run/podman.sock",
        ] {
            let config = filesystem_config(Filesystem {
                binds: Some(vec![FilesystemBind {
                    source: source.to_string(),
                    target: "/data".to_string(),
                    read_only: None,
                }]),
                ..Filesystem::default()
            });

            let error = resolve(&config).unwrap_err();

            assert!(error
                .to_string()
                .contains("config.filesystem.binds[0].source"));
        }
    }

    #[test]
    fn managed_volume_defaults_writable_and_preserves_name() {
        let config = filesystem_config(Filesystem {
            volumes: Some(vec![FilesystemVolume {
                name: Some("database-1".to_string()),
                target: "/data".to_string(),
                read_only: None,
                source: None,
                mode: None,
            }]),
            ..Filesystem::default()
        });

        let resolved = resolve(&config).unwrap();
        let volume = &resolved.filesystem.unwrap().volumes.unwrap()[0];

        assert_eq!(volume.name.as_deref(), Some("database-1"));
        assert!(!volume.read_only);
    }

    #[test]
    fn anonymous_volume_rejects_read_only_access() {
        let config = filesystem_config(Filesystem {
            volumes: Some(vec![FilesystemVolume {
                name: None,
                target: "/data".to_string(),
                read_only: Some(true),
                source: None,
                mode: None,
            }]),
            ..Filesystem::default()
        });

        let error = resolve(&config).unwrap_err();

        assert!(error.to_string().contains(
            "config.filesystem.volumes[0].readOnly cannot be true for an anonymous volume"
        ));
    }

    #[test]
    fn invalid_managed_volume_names_return_indexed_errors() {
        for name in ["", ".hidden", "path/name", "name:tag", "unit.volume"] {
            let config = filesystem_config(Filesystem {
                volumes: Some(vec![FilesystemVolume {
                    name: Some(name.to_string()),
                    target: "/data".to_string(),
                    read_only: None,
                    source: None,
                    mode: None,
                }]),
                ..Filesystem::default()
            });

            let error = resolve(&config).unwrap_err();

            assert!(error
                .to_string()
                .contains("config.filesystem.volumes[0].name"));
        }
    }

    #[test]
    fn legacy_volume_fields_return_migration_errors() {
        let cases = [
            (Some("/srv/data"), None, "[[config.filesystem.binds]]"),
            (Some("./data"), None, "reviewed absolute host path"),
            (Some("database"), None, "volumes[].name"),
            (None, Some("ro"), "readOnly"),
        ];

        for (source, mode, expected) in cases {
            let config = filesystem_config(Filesystem {
                volumes: Some(vec![FilesystemVolume {
                    name: None,
                    target: "/data".to_string(),
                    read_only: None,
                    source: source.map(str::to_string),
                    mode: mode.map(str::to_string),
                }]),
                ..Filesystem::default()
            });

            let error = resolve(&config).unwrap_err();

            assert!(error.to_string().contains(expected));
        }
    }

    #[test]
    fn protected_mount_targets_return_field_specific_errors() {
        for target in [
            "/",
            "/nix",
            "/nix/store",
            "/nix/store/pkg",
            "/dev",
            "/proc/sys",
        ] {
            let config = filesystem_config(Filesystem {
                volumes: Some(vec![FilesystemVolume {
                    name: None,
                    target: target.to_string(),
                    read_only: None,
                    source: None,
                    mode: None,
                }]),
                ..Filesystem::default()
            });

            let error = resolve(&config).unwrap_err();

            assert!(error
                .to_string()
                .contains("config.filesystem.volumes[0].target"));
        }
    }

    #[test]
    fn only_tmpfs_can_target_runtime_temporary_trees() {
        let volume_config = filesystem_config(Filesystem {
            volumes: Some(vec![FilesystemVolume {
                name: None,
                target: "/tmp/cache".to_string(),
                read_only: None,
                source: None,
                mode: None,
            }]),
            ..Filesystem::default()
        });
        let tmpfs_config = filesystem_config(Filesystem {
            tmpfs: Some(vec![FilesystemTmpfsInput::Typed(FilesystemTmpfs {
                target: "/tmp/cache".to_string(),
                mode: None,
                size: None,
            })]),
            ..Filesystem::default()
        });

        assert!(resolve(&volume_config).is_err());
        assert!(resolve(&tmpfs_config).is_ok());
    }

    #[test]
    fn duplicate_and_nested_same_kind_targets_return_errors() {
        let cases = [
            (Some(("/cache", "/cache")), None, None),
            (None, Some(("/cache", "/cache/nested")), None),
            (None, None, Some(("/cache", "/cache/nested"))),
        ];

        for (tmpfs_targets, bind_targets, volume_targets) in cases {
            let tmpfs = tmpfs_targets.map(|(left, right)| {
                [left, right]
                    .into_iter()
                    .map(|target| {
                        FilesystemTmpfsInput::Typed(FilesystemTmpfs {
                            target: target.to_string(),
                            mode: None,
                            size: None,
                        })
                    })
                    .collect()
            });
            let binds = bind_targets.map(|(left, right)| {
                [left, right]
                    .into_iter()
                    .enumerate()
                    .map(|(index, target)| FilesystemBind {
                        source: format!("/srv/source-{index}"),
                        target: target.to_string(),
                        read_only: None,
                    })
                    .collect()
            });
            let volumes = volume_targets.map(|(left, right)| {
                [left, right]
                    .into_iter()
                    .map(|target| FilesystemVolume {
                        name: None,
                        target: target.to_string(),
                        read_only: None,
                        source: None,
                        mode: None,
                    })
                    .collect()
            });
            let config = filesystem_config(Filesystem {
                tmpfs,
                binds,
                volumes,
                ..Filesystem::default()
            });

            let error = resolve(&config).unwrap_err();

            assert!(error.to_string().contains("overlaps"));
        }
    }

    #[test]
    fn nested_cross_kind_targets_return_errors() {
        let cross_kind_cases = [(true, false), (false, true)];
        for (include_bind, include_volume) in cross_kind_cases {
            let config = filesystem_config(Filesystem {
                tmpfs: Some(vec![FilesystemTmpfsInput::Typed(FilesystemTmpfs {
                    target: "/cache".to_string(),
                    mode: None,
                    size: None,
                })]),
                binds: include_bind.then(|| {
                    vec![FilesystemBind {
                        source: "/srv/cache".to_string(),
                        target: "/cache/nested".to_string(),
                        read_only: None,
                    }]
                }),
                volumes: include_volume.then(|| {
                    vec![FilesystemVolume {
                        name: None,
                        target: if include_bind {
                            "/state"
                        } else {
                            "/cache/nested"
                        }
                        .to_string(),
                        read_only: None,
                        source: None,
                        mode: None,
                    }]
                }),
                ..Filesystem::default()
            });

            let error = resolve(&config).unwrap_err();

            assert!(error.to_string().contains("overlaps"));
        }

        let bind_volume_config = filesystem_config(Filesystem {
            binds: Some(vec![FilesystemBind {
                source: "/srv/cache".to_string(),
                target: "/cache".to_string(),
                read_only: None,
            }]),
            volumes: Some(vec![FilesystemVolume {
                name: None,
                target: "/cache/nested".to_string(),
                read_only: None,
                source: None,
                mode: None,
            }]),
            ..Filesystem::default()
        });

        assert!(resolve(&bind_volume_config)
            .unwrap_err()
            .to_string()
            .contains("overlaps"));
    }

    #[test]
    fn empty_devices_are_omitted() {
        let config = filesystem_config(Filesystem {
            devices: Some(Vec::new()),
            ..Filesystem::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(resolved.filesystem.unwrap().devices, None);
    }

    #[test]
    fn cdi_device_references_preserve_declaration_order() {
        let config = filesystem_config(Filesystem {
            devices: Some(vec![
                Device {
                    source: "nvidia.com/gpu=all".to_string(),
                    target: None,
                    permissions: None,
                },
                Device {
                    source: "vendor.example/device_class=device-1.2".to_string(),
                    target: None,
                    permissions: None,
                },
            ]),
            ..Filesystem::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.filesystem,
            Some(ResolvedFilesystem {
                read_only: Some(true),
                tmpfs: None,
                binds: None,
                volumes: None,
                devices: Some(vec![
                    ResolvedDevice {
                        source: "nvidia.com/gpu=all".to_string(),
                    },
                    ResolvedDevice {
                        source: "vendor.example/device_class=device-1.2".to_string(),
                    },
                ]),
            })
        );
        let json = serde_json::to_value(&resolved).unwrap();
        assert_eq!(
            json["filesystem"]["devices"][0]["source"],
            "nvidia.com/gpu=all"
        );
        assert_eq!(
            json["filesystem"]["devices"][1]["source"],
            "vendor.example/device_class=device-1.2"
        );
        assert_eq!(json["filesystem"]["devices"][0].get("target"), None);
        assert_eq!(json["filesystem"]["devices"][0].get("permissions"), None);
    }

    #[test]
    fn read_only_tmpfs_volumes_and_cdi_devices_resolve_together() {
        let config = filesystem_config(Filesystem {
            read_only: Some(true),
            tmpfs: Some(vec![FilesystemTmpfsInput::Typed(FilesystemTmpfs {
                target: "/run/graft".to_string(),
                mode: None,
                size: None,
            })]),
            volumes: Some(vec![FilesystemVolume {
                name: None,
                target: "/data".to_string(),
                read_only: None,
                source: None,
                mode: None,
            }]),
            devices: Some(vec![Device {
                source: "nvidia.com/gpu=all".to_string(),
                target: None,
                permissions: None,
            }]),
            ..Filesystem::default()
        });

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.filesystem,
            Some(ResolvedFilesystem {
                read_only: Some(true),
                tmpfs: Some(vec![ResolvedFilesystemTmpfs {
                    target: "/run/graft".to_string(),
                    mode: None,
                    size: None,
                }]),
                binds: None,
                volumes: Some(vec![ResolvedFilesystemVolume {
                    name: None,
                    target: "/data".to_string(),
                    read_only: false,
                }]),
                devices: Some(vec![ResolvedDevice {
                    source: "nvidia.com/gpu=all".to_string(),
                }]),
            })
        );
    }

    #[test]
    fn malformed_cdi_device_sources_return_field_specific_errors() {
        let cases = [
            ("", "config.filesystem.devices[0].source cannot be empty"),
            ("  ", "config.filesystem.devices[0].source cannot be empty"),
            (
                "nvidia.com/gpu=al\nl",
                "config.filesystem.devices[0].source cannot contain control characters",
            ),
            (
                "/dev/nvidia0",
                "config.filesystem.devices[0].source must be a CDI qualified name in vendor/class=device form",
            ),
            (
                "nvidia.com/gpu",
                "config.filesystem.devices[0].source must be a CDI qualified name in vendor/class=device form",
            ),
            (
                "nvidia.com=all",
                "config.filesystem.devices[0].source must be a CDI qualified name in vendor/class=device form",
            ),
            (
                "nvidia.com/gpu/extra=all",
                "config.filesystem.devices[0].source must be a CDI qualified name in vendor/class=device form",
            ),
            (
                "nvidia.com/gpu=all=extra",
                "config.filesystem.devices[0].source must be a CDI qualified name in vendor/class=device form",
            ),
            (
                "_nvidia/gpu=all",
                "config.filesystem.devices[0].source must be a CDI qualified name in vendor/class=device form",
            ),
            (
                "a/gpu=all",
                "config.filesystem.devices[0].source must be a CDI qualified name in vendor/class=device form",
            ),
            (
                "nvidia.com/g=all",
                "config.filesystem.devices[0].source must be a CDI qualified name in vendor/class=device form",
            ),
            (
                "nvidia.com/gpu_=all",
                "config.filesystem.devices[0].source must be a CDI qualified name in vendor/class=device form",
            ),
            (
                "nvidia.com/gpu=-all",
                "config.filesystem.devices[0].source must be a CDI qualified name in vendor/class=device form",
            ),
            (
                "nvidiä.com/gpu=all",
                "config.filesystem.devices[0].source must be a CDI qualified name in vendor/class=device form",
            ),
            (
                "nvidia.com/gpu=device:variant",
                "config.filesystem.devices[0].source cannot contain ':'",
            ),
        ];

        for (source, expected) in cases {
            let config = filesystem_config(Filesystem {
                devices: Some(vec![Device {
                    source: source.to_string(),
                    target: None,
                    permissions: None,
                }]),
                ..Filesystem::default()
            });

            let error = resolve(&config).unwrap_err();

            assert_eq!(
                error.to_string(),
                expected,
                "unexpected diagnostic for {source:?}"
            );
        }
    }

    #[test]
    fn duplicate_cdi_device_source_returns_indexed_error() {
        let config = filesystem_config(Filesystem {
            devices: Some(vec![
                Device {
                    source: "nvidia.com/gpu=all".to_string(),
                    target: None,
                    permissions: None,
                },
                Device {
                    source: "nvidia.com/gpu=all".to_string(),
                    target: None,
                    permissions: None,
                },
            ]),
            ..Filesystem::default()
        });

        let error = resolve(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "config.filesystem.devices[1].source duplicates an earlier CDI reference"
        );
    }

    #[test]
    fn cdi_device_target_remapping_returns_indexed_error() {
        let config = filesystem_config(Filesystem {
            devices: Some(vec![Device {
                source: "nvidia.com/gpu=all".to_string(),
                target: Some("/dev/gpu0".to_string()),
                permissions: None,
            }]),
            ..Filesystem::default()
        });

        let error = resolve(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "config.filesystem.devices[0].target is configured but CDI target remapping is not supported"
        );
    }

    #[test]
    fn cdi_device_permissions_return_indexed_error() {
        let config = filesystem_config(Filesystem {
            devices: Some(vec![Device {
                source: "nvidia.com/gpu=all".to_string(),
                target: None,
                permissions: Some("rwm".to_string()),
            }]),
            ..Filesystem::default()
        });

        let error = resolve(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "config.filesystem.devices[0].permissions is configured but CDI permissions are not supported"
        );
    }

    #[test]
    fn minimal_config_receives_secure_defaults() {
        let resolved = resolve(&named_config()).unwrap();

        assert_eq!(resolved.filesystem.unwrap().read_only, Some(true));
        assert_eq!(
            resolved.security,
            Some(ResolvedSecurity {
                drop_capabilities: Some(vec!["all".to_string()]),
                add_capabilities: None,
                no_new_privileges: Some(true),
            })
        );
    }

    #[test]
    fn explicit_hardening_controls_are_resolved() {
        let config = config_with_toml(
            "[config.filesystem]\nreadOnly = true\n\n\
             [config.security]\ndropCapabilities = [\"all\"]\nnoNewPrivileges = true",
        );

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.filesystem,
            Some(ResolvedFilesystem {
                read_only: Some(true),
                tmpfs: None,
                binds: None,
                volumes: None,
                devices: None,
            })
        );
        assert_eq!(
            resolved.security,
            Some(ResolvedSecurity {
                drop_capabilities: Some(vec!["all".to_string()]),
                add_capabilities: None,
                no_new_privileges: Some(true),
            })
        );
        let json = serde_json::to_value(&resolved).unwrap();
        assert_eq!(json["filesystem"]["readOnly"], true);
        assert_eq!(json["security"]["dropCapabilities"][0], "all");
        assert_eq!(json["security"]["noNewPrivileges"], true);
    }

    #[test]
    fn capability_addition_order_is_preserved() {
        let config = config_with_toml(
            "[config.security]\n\
             addCapabilities = [\"CAP_NET_BIND_SERVICE\", \"CAP_CHOWN\"]",
        );

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.security,
            Some(ResolvedSecurity {
                drop_capabilities: Some(vec!["all".to_string()]),
                add_capabilities: Some(vec![
                    "CAP_NET_BIND_SERVICE".to_string(),
                    "CAP_CHOWN".to_string(),
                ]),
                no_new_privileges: Some(true),
            })
        );
    }

    #[test]
    fn explicit_false_values_resolve_as_typed_relaxations() {
        let config = config_with_toml(
            "[config.filesystem]\nreadOnly = false\n\n\
             [config.security]\nnoNewPrivileges = false",
        );

        let resolved = resolve(&config).unwrap();

        assert_eq!(resolved.filesystem.unwrap().read_only, Some(false));
        assert_eq!(resolved.security.unwrap().no_new_privileges, Some(false));
    }

    #[test]
    fn invalid_capability_drop_entries_return_specific_errors() {
        let cases = [
            ("[]", "config.security.dropCapabilities cannot be empty"),
            (
                "[\"CAP_CHOWN\"]",
                "config.security.dropCapabilities must be [\"all\"]; use config.security.addCapabilities for required capabilities",
            ),
            (
                "[\"all\", \"CAP_CHOWN\"]",
                "config.security.dropCapabilities must be [\"all\"]; use config.security.addCapabilities for required capabilities",
            ),
        ];

        for (entries, expected) in cases {
            let config =
                config_with_toml(&format!("[config.security]\ndropCapabilities = {entries}"));

            let error = resolve(&config).unwrap_err();

            assert_eq!(
                error.to_string(),
                expected,
                "unexpected diagnostic for {entries}"
            );
        }
    }

    #[test]
    fn invalid_capability_additions_return_specific_errors() {
        let cases = [
            ("[]", "config.security.addCapabilities cannot be empty"),
            (
                "[\"all\"]",
                "config.security.addCapabilities[0] must be a canonical CAP_* name",
            ),
            (
                "[\"cap_chown\"]",
                "config.security.addCapabilities[0] must be a canonical CAP_* name",
            ),
            (
                "[\"CAP_CHOWN\", \"CAP_CHOWN\"]",
                "config.security.addCapabilities[1] duplicates an earlier capability",
            ),
        ];

        for (entries, expected) in cases {
            let config =
                config_with_toml(&format!("[config.security]\naddCapabilities = {entries}"));

            let error = resolve(&config).unwrap_err();

            assert_eq!(
                error.to_string(),
                expected,
                "unexpected diagnostic for {entries}"
            );
        }
    }

    #[test]
    fn external_unit_dependencies_resolve_to_sorted_concrete_relations() {
        let mut config = named_config();
        config.dependencies = Some(vec![
            external_unit_dependency(
                "zeta.service",
                Some(DependencyRequirement::Required),
                Some(DependencyOrdering::After),
                None,
            ),
            external_unit_dependency(
                "foreign.target",
                Some(DependencyRequirement::Optional),
                Some(DependencyOrdering::Before),
                None,
            ),
            external_unit_dependency(
                "bound.service",
                None,
                None,
                Some(DependencyLifecycle::Bound),
            ),
            external_unit_dependency(
                "alpha.service",
                Some(DependencyRequirement::Required),
                Some(DependencyOrdering::After),
                Some(DependencyLifecycle::PartOf),
            ),
        ]);

        let resolved = resolve(&config).unwrap();

        assert_eq!(
            resolved.dependencies,
            Some(ResolvedDependencies {
                requires: vec!["alpha.service".to_string(), "zeta.service".to_string()],
                wants: vec!["foreign.target".to_string()],
                after: vec!["alpha.service".to_string(), "zeta.service".to_string()],
                before: vec!["foreign.target".to_string()],
                part_of: vec!["alpha.service".to_string()],
                binds_to: vec!["bound.service".to_string()],
            })
        );
        let json = serde_json::to_value(&resolved).unwrap();
        assert_eq!(json["dependencies"]["partOf"][0], "alpha.service");
        assert_eq!(json["dependencies"]["bindsTo"][0], "bound.service");
    }

    #[test]
    fn empty_dependencies_are_omitted() {
        let mut config = named_config();
        config.dependencies = Some(Vec::new());

        let resolved = resolve(&config).unwrap();

        assert_eq!(resolved.dependencies, None);
        let json = serde_json::to_value(&resolved).unwrap();
        assert_eq!(json.get("dependencies"), None);
    }

    #[test]
    fn dependency_without_relationship_returns_error() {
        let mut config = named_config();
        config.dependencies = Some(vec![external_unit_dependency(
            "database.service",
            None,
            None,
            None,
        )]);

        let error = resolve(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "dependency target 'externalUnit:database.service' must configure at least one relationship"
        );
    }

    #[test]
    fn bound_lifecycle_rejects_separate_requirement_axis() {
        let mut config = named_config();
        config.dependencies = Some(vec![external_unit_dependency(
            "database.service",
            Some(DependencyRequirement::Optional),
            None,
            Some(DependencyLifecycle::Bound),
        )]);

        let error = resolve(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "dependency target 'externalUnit:database.service' cannot combine requirement with lifecycle = \"bound\"; BindsTo already activates the target"
        );
    }

    #[test]
    fn concrete_external_systemd_unit_names_are_accepted() {
        for unit in [
            "postgresql.service",
            "worker@one.service",
            "api.socket",
            "dev-disk.device",
            "-.mount",
            "data.automount",
            "paging.swap",
            "network-online.target",
            "watch.path",
            "schedule.timer",
            "work.slice",
            "transient.scope",
        ] {
            let mut config = named_config();
            config.dependencies = Some(vec![external_unit_dependency(
                unit,
                Some(DependencyRequirement::Required),
                None,
                None,
            )]);

            let resolved = resolve(&config).unwrap();

            assert_eq!(resolved.dependencies.unwrap().requires, [unit.to_string()]);
        }
    }

    #[test]
    fn invalid_external_systemd_unit_names_return_specific_errors() {
        let cases = [
            ("", "dependency externalUnit cannot be empty"),
            (
                "database",
                "dependency externalUnit must include a supported systemd unit suffix",
            ),
            (
                "database.invalid",
                "dependency externalUnit must include a supported systemd unit suffix",
            ),
            (
                "database@.service",
                "dependency externalUnit must name a concrete non-template unit",
            ),
            (
                "database@one@two.service",
                "dependency externalUnit must name a concrete non-template unit",
            ),
            (
                "database/path.service",
                "dependency externalUnit contains unsupported characters",
            ),
            (
                "database %n.service",
                "dependency externalUnit contains unsupported characters",
            ),
            (
                "dátabase.service",
                "dependency externalUnit contains unsupported characters",
            ),
        ];

        for (unit, expected) in cases {
            let mut config = named_config();
            config.dependencies = Some(vec![external_unit_dependency(
                unit,
                Some(DependencyRequirement::Required),
                None,
                None,
            )]);

            let error = resolve(&config).unwrap_err();

            assert_eq!(error.to_string(), expected);
        }
    }

    #[test]
    fn overlong_external_systemd_unit_name_returns_error() {
        let unit = format!("{}.service", "a".repeat(248));
        let mut config = named_config();
        config.dependencies = Some(vec![external_unit_dependency(
            &unit,
            Some(DependencyRequirement::Required),
            None,
            None,
        )]);

        let error = resolve(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "dependency externalUnit cannot exceed 255 characters"
        );
    }

    #[test]
    fn workload_dependency_resolves_quadlet_source_unit_from_context() {
        let mut worker =
            contextual_workload("worker", ResolvedDeployTarget::User, None, None, None);
        worker.dependencies = Some(vec![workload_dependency(
            "database",
            Some(DependencyRequirement::Required),
            Some(DependencyOrdering::After),
            Some(DependencyLifecycle::PartOf),
        )]);
        let database =
            contextual_workload("database", ResolvedDeployTarget::User, None, None, None);
        let sources = [
            ConfigSource::new("worker", &worker),
            ConfigSource::new("database-source", &database),
        ];

        let resolved = resolve_with_context(&worker, &sources).unwrap();

        assert_eq!(
            resolved.dependencies,
            Some(ResolvedDependencies {
                requires: vec!["database-source.container".to_string()],
                wants: Vec::new(),
                after: vec!["database-source.container".to_string()],
                before: Vec::new(),
                part_of: vec!["database-source.container".to_string()],
                binds_to: Vec::new(),
            })
        );
    }

    #[test]
    fn finite_workloads_can_be_dependency_targets() {
        for lifecycle in [ServiceLifecycle::Job, ServiceLifecycle::Setup] {
            let mut worker =
                contextual_workload("worker", ResolvedDeployTarget::System, None, None, None);
            worker.dependencies = Some(vec![workload_dependency(
                "prerequisite",
                Some(DependencyRequirement::Required),
                Some(DependencyOrdering::After),
                None,
            )]);
            let mut prerequisite = contextual_workload(
                "prerequisite",
                ResolvedDeployTarget::System,
                None,
                Some(lifecycle),
                None,
            );
            prerequisite.config.as_mut().unwrap().runtime = Some(Runtime {
                command: Some(vec!["/bin/true".to_string()]),
                ..Runtime::default()
            });
            let sources = [
                ConfigSource::new("worker", &worker),
                ConfigSource::new("prerequisite", &prerequisite),
            ];

            let resolved = resolve_set(&sources).unwrap();

            assert_eq!(resolved.len(), 2);
            assert_eq!(
                resolved[0].dependencies.as_ref().unwrap().after,
                ["prerequisite.container".to_string()]
            );
        }
    }

    #[test]
    fn bound_dependency_rejects_job_target() {
        let mut worker =
            contextual_workload("worker", ResolvedDeployTarget::System, None, None, None);
        worker.dependencies = Some(vec![workload_dependency(
            "prerequisite",
            None,
            Some(DependencyOrdering::After),
            Some(DependencyLifecycle::Bound),
        )]);
        let mut prerequisite = contextual_workload(
            "prerequisite",
            ResolvedDeployTarget::System,
            None,
            Some(ServiceLifecycle::Job),
            None,
        );
        prerequisite.config.as_mut().unwrap().runtime = Some(Runtime {
            command: Some(vec!["/bin/true".to_string()]),
            ..Runtime::default()
        });
        let sources = [
            ConfigSource::with_origin("worker", "worker.toml", &worker),
            ConfigSource::with_origin("prerequisite", "prerequisite.toml", &prerequisite),
        ];

        let error = resolve_with_context(&worker, &sources).unwrap_err();

        assert_eq!(
            error.to_string(),
            "dependency workload reference 'prerequisite' for workload 'worker' cannot bind to the inactive result of a job lifecycle in config context worker.toml"
        );
    }

    #[test]
    fn workload_dependency_requires_explicit_context() {
        let mut worker = named_config();
        worker.dependencies = Some(vec![workload_dependency(
            "database",
            Some(DependencyRequirement::Required),
            None,
            None,
        )]);

        let error = resolve(&worker).unwrap_err();

        assert_eq!(
            error.to_string(),
            "workload dependencies require explicit workload context"
        );
    }

    #[test]
    fn unsafe_workload_dependency_name_returns_error() {
        let mut worker = named_config();
        worker.dependencies = Some(vec![workload_dependency(
            "bad/name",
            Some(DependencyRequirement::Required),
            None,
            None,
        )]);
        let sources = [ConfigSource::new("worker", &worker)];

        let error = resolve_with_context(&worker, &sources).unwrap_err();
        let diagnostic = format!("{error:#}");

        assert!(
            diagnostic.contains("dependency workload reference contains unsupported characters")
        );
    }

    #[test]
    fn invalid_workload_dependency_relationships_return_errors() {
        let cases = [
            (
                "missing",
                None,
                ResolvedDeployTarget::System,
                "was not found",
            ),
            (
                "disabled",
                Some(false),
                ResolvedDeployTarget::System,
                "is disabled",
            ),
            (
                "other-target",
                None,
                ResolvedDeployTarget::User,
                "has a different deploy target",
            ),
        ];

        for (name, enable, target, expected) in cases {
            let mut worker =
                contextual_workload("worker", ResolvedDeployTarget::System, None, None, None);
            worker.dependencies = Some(vec![workload_dependency(
                name,
                Some(DependencyRequirement::Required),
                None,
                None,
            )]);
            let dependency_name = if name == "missing" { "present" } else { name };
            let dependency = contextual_workload(dependency_name, target, enable, None, None);
            let sources = [
                ConfigSource::with_origin("worker", "worker.toml", &worker),
                ConfigSource::with_origin(dependency_name, "dependency.toml", &dependency),
            ];

            let error = resolve_with_context(&worker, &sources).unwrap_err();

            assert!(error.to_string().contains(expected));
            assert!(error.to_string().contains("worker.toml"));
        }
    }

    #[test]
    fn self_workload_dependency_returns_error() {
        let mut worker =
            contextual_workload("worker", ResolvedDeployTarget::System, None, None, None);
        worker.dependencies = Some(vec![workload_dependency(
            "worker",
            Some(DependencyRequirement::Required),
            None,
            None,
        )]);
        let sources = [ConfigSource::with_origin("worker", "worker.toml", &worker)];

        let error = resolve_with_context(&worker, &sources).unwrap_err();

        assert_eq!(
            error.to_string(),
            "workload 'worker' cannot depend on itself in config context worker.toml"
        );
    }

    #[test]
    fn duplicate_dependency_target_returns_error() {
        let mut config = named_config();
        config.dependencies = Some(vec![
            external_unit_dependency(
                "database.service",
                Some(DependencyRequirement::Required),
                None,
                None,
            ),
            external_unit_dependency(
                "database.service",
                None,
                Some(DependencyOrdering::After),
                None,
            ),
        ]);

        let error = resolve(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "duplicate dependency target 'externalUnit:database.service'"
        );
    }

    #[test]
    fn dependency_targets_resolving_to_same_service_return_error() {
        let mut worker =
            contextual_workload("worker", ResolvedDeployTarget::System, None, None, None);
        worker.dependencies = Some(vec![
            workload_dependency(
                "database",
                Some(DependencyRequirement::Required),
                None,
                None,
            ),
            external_unit_dependency(
                "database.service",
                None,
                Some(DependencyOrdering::After),
                None,
            ),
        ]);
        let database =
            contextual_workload("database", ResolvedDeployTarget::System, None, None, None);
        let sources = [
            ConfigSource::new("worker", &worker),
            ConfigSource::new("database", &database),
        ];

        let error = resolve_with_context(&worker, &sources).unwrap_err();

        assert_eq!(
            error.to_string(),
            "multiple dependency targets resolve to unit 'database.service'"
        );
    }

    #[test]
    fn workload_dependency_cycle_returns_path() {
        let mut first =
            contextual_workload("first", ResolvedDeployTarget::System, None, None, None);
        first.dependencies = Some(vec![workload_dependency(
            "second",
            Some(DependencyRequirement::Required),
            None,
            None,
        )]);
        let mut second =
            contextual_workload("second", ResolvedDeployTarget::System, None, None, None);
        second.dependencies = Some(vec![workload_dependency(
            "first",
            None,
            Some(DependencyOrdering::After),
            None,
        )]);
        let sources = [
            ConfigSource::new("first", &first),
            ConfigSource::new("second", &second),
        ];

        let error = resolve_with_context(&first, &sources).unwrap_err();

        assert_eq!(
            error.to_string(),
            "workload dependency cycle: first (first) -> second (second) -> first (first)"
        );
    }

    #[test]
    fn mixed_network_and_workload_dependency_cycle_returns_error() {
        let first = contextual_workload(
            "first",
            ResolvedDeployTarget::System,
            None,
            None,
            Some(container_network("second")),
        );
        let mut second =
            contextual_workload("second", ResolvedDeployTarget::System, None, None, None);
        second.dependencies = Some(vec![workload_dependency(
            "first",
            Some(DependencyRequirement::Optional),
            None,
            None,
        )]);
        let sources = [
            ConfigSource::new("first", &first),
            ConfigSource::new("second", &second),
        ];

        let error = resolve_with_context(&first, &sources).unwrap_err();

        assert_eq!(
            error.to_string(),
            "workload dependency cycle: first (first) -> second (second) -> first (first)"
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
                deploy: Some(Deploy {
                    target: Some(DeployTarget::System),
                    ..Deploy::default()
                }),
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
                target: Some(DeployTarget::System),
                ..Deploy::default()
            }),
            ..ContainerConfig::default()
        };

        let resolved = resolve(&config).unwrap();

        assert_eq!(resolved.deploy.enable, Some(false));
    }

    #[test]
    fn startup_activation_maps_to_fixed_install_targets() {
        for (target, expected_deploy_target, expected_install_target, expected_json) in [
            (
                Some(DeployTarget::System),
                ResolvedDeployTarget::System,
                ResolvedInstallTarget::MultiUser,
                "multi-user.target",
            ),
            (
                Some(DeployTarget::User),
                ResolvedDeployTarget::User,
                ResolvedInstallTarget::Default,
                "default.target",
            ),
        ] {
            let config = ContainerConfig {
                version: Some(SUPPORTED_VERSION),
                name: Some("dev".to_string()),
                deploy: Some(Deploy {
                    target,
                    activation: Some(DeployActivation::Startup),
                    ..Deploy::default()
                }),
                ..ContainerConfig::default()
            };

            let resolved = resolve(&config).unwrap();

            assert_eq!(resolved.deploy.target, expected_deploy_target);
            assert_eq!(
                resolved.install,
                Some(ResolvedInstall {
                    wanted_by: expected_install_target,
                })
            );
            let json = serde_json::to_value(&resolved).unwrap();
            assert_eq!(json["install"]["wantedBy"], expected_json);
        }
    }

    #[test]
    fn disabled_startup_activation_remains_resolved_dormant_intent() {
        let config = ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            name: Some("dev".to_string()),
            deploy: Some(Deploy {
                enable: Some(false),
                target: Some(DeployTarget::System),
                activation: Some(DeployActivation::Startup),
            }),
            ..ContainerConfig::default()
        };

        let resolved = resolve(&config).unwrap();

        assert_eq!(resolved.deploy.enable, Some(false));
        assert_eq!(
            resolved.install,
            Some(ResolvedInstall {
                wanted_by: ResolvedInstallTarget::MultiUser,
            })
        );
    }

    #[test]
    fn startup_activation_accepts_all_service_lifecycles() {
        for lifecycle in [
            ServiceLifecycle::LongRunning,
            ServiceLifecycle::Job,
            ServiceLifecycle::Setup,
        ] {
            let mut config = if lifecycle == ServiceLifecycle::LongRunning {
                service_config(Service {
                    lifecycle: Some(lifecycle),
                    ..Service::default()
                })
            } else {
                finite_service_config(Service {
                    lifecycle: Some(lifecycle),
                    ..Service::default()
                })
            };
            config.deploy = Some(Deploy {
                target: Some(DeployTarget::System),
                activation: Some(DeployActivation::Startup),
                ..Deploy::default()
            });

            let resolved = resolve(&config).unwrap();

            assert_eq!(
                resolved.install,
                Some(ResolvedInstall {
                    wanted_by: ResolvedInstallTarget::MultiUser,
                })
            );
        }
    }

    #[test]
    fn raw_quadlet_install_returns_migration_error() {
        let config = ContainerConfig {
            version: Some(SUPPORTED_VERSION),
            name: Some("dev".to_string()),
            config: Some(Config {
                quadlet: Some(Quadlet {
                    install: Some(HashMap::from([(
                        "WantedBy".to_string(),
                        vec!["default.target".to_string()],
                    )])),
                    ..Quadlet::default()
                }),
                ..Config::default()
            }),
            ..ContainerConfig::default()
        };

        let error = resolve(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "config.quadlet.install is not supported; use deploy.activation = \"startup\""
        );
    }

    #[test]
    fn missing_deploy_target_returns_migration_error() {
        let mut config = named_config();
        config.deploy = None;

        let error = resolve(&config).unwrap_err();

        assert_eq!(
            error.to_string(),
            "deploy.target is required; set deploy.target = \"user\" or deploy.target = \"system\""
        );
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

        assert_eq!(json.get("install"), None);
        assert_eq!(json.get("container"), None);
        assert_eq!(json["filesystem"]["readOnly"], true);
        assert_eq!(json.get("network"), None);
        assert_eq!(
            json["security"]["dropCapabilities"],
            serde_json::json!(["all"])
        );
        assert_eq!(json["security"]["noNewPrivileges"], true);
        assert_eq!(json.get("service"), None);
        assert_eq!(json["deploy"].get("enable"), None);
    }
}
