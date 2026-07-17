//! Typed discovery-document schema and semantic validation.

use std::collections::BTreeSet;

use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use crate::protocol::{
    ManagerKind, ProtocolVersionRange, WorkerTarget, PROTOCOL_MAJOR, PROTOCOL_MAX_MINOR,
};

use super::{canonical, ManifestError};

/// Manifest schema major version supported by this worker.
pub const MANIFEST_SCHEMA_MAJOR: u16 = 1;
/// Manifest schema minimum minor version supported by this worker.
pub const MANIFEST_SCHEMA_MIN_MINOR: u16 = 0;
/// Manifest schema maximum minor version supported by this worker.
pub const MANIFEST_SCHEMA_MAX_MINOR: u16 = 0;
/// Maximum number of workloads in one manifest.
pub const MAX_MANIFEST_WORKLOADS: usize = 1_024;
/// Maximum dependencies in one workload record.
pub const MAX_WORKLOAD_DEPENDENCIES: usize = 256;
/// Maximum bytes in a bounded manifest string.
pub const MAX_MANIFEST_STRING_BYTES: usize = 4 * 1_024;

/// Exact Graft manifest schema version.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ManifestSchemaVersion {
    major: u16,
    minor: u16,
}

impl ManifestSchemaVersion {
    /// Returns the version supported by this worker.
    #[must_use]
    pub const fn current() -> Self {
        Self { major: 1, minor: 0 }
    }

    pub(super) const fn is_compatible(self) -> bool {
        self.major == MANIFEST_SCHEMA_MAJOR && self.minor == MANIFEST_SCHEMA_MAX_MINOR
    }
}

/// Nix package identity shared by a manifest and endpoint descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct ProducerIdentity {
    name: BoundedIdentifier,
    version: BoundedText,
    build_id: BoundedIdentifier,
}

impl ProducerIdentity {
    /// Returns the producer package name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name.0
    }

    /// Returns the producer package version.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version.0
    }

    /// Returns the producer build identity.
    #[must_use]
    pub fn build_id(&self) -> &str {
        &self.build_id.0
    }
}

/// Canonical `UUIDv7` host identity issued by Nix policy.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostIdentifier(Uuid);

impl HostIdentifier {
    /// Returns the canonical lowercase `UUIDv7` string.
    #[must_use]
    pub fn to_canonical_string(self) -> String {
        self.0.hyphenated().to_string()
    }
}

impl Serialize for HostIdentifier {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.hyphenated().to_string())
    }
}

impl<'de> Deserialize<'de> for HostIdentifier {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        let parsed = Uuid::parse_str(&raw).map_err(serde::de::Error::custom)?;
        if parsed.get_version_num() != 7
            || parsed.get_variant() != uuid::Variant::RFC4122
            || parsed.hyphenated().to_string() != raw
        {
            return Err(serde::de::Error::custom(
                "host identifier must be a canonical lowercase UUIDv7",
            ));
        }
        Ok(Self(parsed))
    }
}

/// Lowercase hexadecimal SHA-256 identity.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Sha256Identity(String);

impl Sha256Identity {
    pub(super) fn from_computed(value: String) -> Self {
        Self(value)
    }

    /// Returns the lowercase hexadecimal digest.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for Sha256Identity {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.len() != 64
            || !value
                .bytes()
                .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        {
            return Err(serde::de::Error::custom(
                "SHA-256 identity must contain 64 lowercase hexadecimal characters",
            ));
        }
        Ok(Self(value))
    }
}

/// Worker lifecycle behavior declared for a workload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkloadLifecycle {
    /// A long-running service.
    Service,
    /// A finite startup job.
    StartupJob,
    /// A timer-triggered job.
    TimerJob,
}

/// Declarative startup intent.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartupIntent {
    /// Do not start automatically.
    Disabled,
    /// Start with the context's manager target.
    ManagerTarget,
    /// Start only through a timer.
    Timer,
}

/// Lifecycle operation supported by one workload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleCapability {
    /// Start or converge the workload upward.
    Up,
    /// Stop the workload.
    Down,
    /// Restart the workload.
    Restart,
}

/// Observability layer supported by one workload.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObservabilityCapability {
    /// Declarative manifest evidence.
    Manifest,
    /// systemd manager evidence.
    Manager,
    /// Podman runtime evidence.
    Runtime,
    /// Historical or followed logs.
    Logs,
    /// Approved metrics.
    Metrics,
    /// Storage accounting.
    Storage,
}

/// Runtime backend required by a workload record.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RuntimeBackend {
    /// Rootful or rootless Podman selected by the worker context.
    Podman,
}

/// Minimum runtime-backend compatibility requirement.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct BackendRequirement {
    runtime: RuntimeBackend,
    minimum_version: BoundedText,
}

impl BackendRequirement {
    /// Returns the required runtime backend.
    #[must_use]
    pub const fn runtime(&self) -> RuntimeBackend {
        self.runtime
    }

    /// Returns the minimum compatible backend version.
    #[must_use]
    pub fn minimum_version(&self) -> &str {
        &self.minimum_version.0
    }
}

/// Materialized workload record consumed by the worker.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct WorkloadRecord {
    workload_id: Sha256Identity,
    name: WorkloadName,
    target: WorkerTarget,
    enabled: bool,
    lifecycle: WorkloadLifecycle,
    startup_intent: StartupIntent,
    source_identity: BoundedIdentifier,
    source_digest: Sha256Identity,
    resolved_digest: Sha256Identity,
    dependency_digest: Sha256Identity,
    quadlet_source_unit: SourceUnitName,
    generated_service: ServiceUnitName,
    container_name: WorkloadName,
    artifact_identity: Sha256Identity,
    rootfs_store_path: NixStorePath,
    closure_identity: Sha256Identity,
    dependency_services: Vec<ServiceUnitName>,
    lifecycle_capabilities: Vec<LifecycleCapability>,
    observability_capabilities: Vec<ObservabilityCapability>,
    required_worker_api: ProtocolVersionRange,
    required_producer: ProducerIdentity,
    required_backend: BackendRequirement,
}

/// Validated manifest envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Manifest {
    schema_version: ManifestSchemaVersion,
    worker_api_range: ProtocolVersionRange,
    producer: ProducerIdentity,
    host_id: HostIdentifier,
    target: WorkerTarget,
    manager: ManagerKind,
    generation_id: Sha256Identity,
    #[serde(rename = "manifestDigest")]
    digest: Sha256Identity,
    workload_count: u32,
    workloads: Vec<WorkloadRecord>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct ManifestPreimage<'a> {
    schema_version: ManifestSchemaVersion,
    worker_api_range: ProtocolVersionRange,
    producer: &'a ProducerIdentity,
    host_id: HostIdentifier,
    target: WorkerTarget,
    manager: ManagerKind,
    workload_count: u32,
    workloads: &'a [WorkloadRecord],
}

impl Manifest {
    /// Parses and fully validates one manifest document.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed JSON, incompatible schema/context,
    /// nondeterministic records, invalid cardinality, or digest mismatch.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ManifestError> {
        if u64::try_from(bytes.len()).map_or(true, |length| length > super::MAX_MANIFEST_BYTES) {
            return Err(ManifestError::DocumentTooLarge);
        }
        let manifest: Self = serde_json::from_slice(bytes).map_err(ManifestError::ManifestJson)?;
        manifest.validate()?;
        if canonical::to_canonical_json(&manifest)? != bytes {
            return Err(ManifestError::NonCanonicalJson);
        }
        Ok(manifest)
    }

    /// Returns the manifest generation identity.
    #[must_use]
    pub fn generation_id(&self) -> &Sha256Identity {
        &self.generation_id
    }

    /// Returns the manifest target.
    #[must_use]
    pub const fn target(&self) -> WorkerTarget {
        self.target
    }

    /// Returns the manifest manager kind.
    #[must_use]
    pub const fn manager(&self) -> ManagerKind {
        self.manager
    }

    /// Returns the ordered workload records.
    #[must_use]
    pub fn workloads(&self) -> &[WorkloadRecord] {
        &self.workloads
    }

    /// Returns the declared workload count.
    #[must_use]
    pub const fn workload_count(&self) -> u32 {
        self.workload_count
    }

    /// Returns the validated manifest digest.
    #[must_use]
    pub fn digest(&self) -> &Sha256Identity {
        &self.digest
    }

    /// Returns the Nix-issued host identity.
    #[must_use]
    pub const fn host_id(&self) -> HostIdentifier {
        self.host_id
    }

    /// Returns the manifest producer identity.
    #[must_use]
    pub const fn producer(&self) -> &ProducerIdentity {
        &self.producer
    }

    /// Returns the compatible worker API range.
    #[must_use]
    pub const fn api_range(&self) -> ProtocolVersionRange {
        self.worker_api_range
    }

    fn validate(&self) -> Result<(), ManifestError> {
        if !self.schema_version.is_compatible() {
            return Err(ManifestError::IncompatibleSchema);
        }
        validate_context(self.target, self.manager)?;
        validate_api_compatibility(self.worker_api_range)?;
        if self.workloads.len() > MAX_MANIFEST_WORKLOADS
            || usize::try_from(self.workload_count).ok() != Some(self.workloads.len())
        {
            return Err(ManifestError::WorkloadCount);
        }

        let mut ids = BTreeSet::new();
        let mut names = BTreeSet::new();
        let mut sources = BTreeSet::new();
        let mut services = BTreeSet::new();
        let mut containers = BTreeSet::new();
        let mut previous = None;
        for workload in &self.workloads {
            workload.validate(self.target, self.worker_api_range, &self.producer)?;
            if previous.is_some_and(|name| name >= &workload.name) {
                return Err(ManifestError::WorkloadOrder);
            }
            previous = Some(&workload.name);
            if !ids.insert(&workload.workload_id)
                || !names.insert(&workload.name)
                || !sources.insert(&workload.quadlet_source_unit)
                || !services.insert(&workload.generated_service)
                || !containers.insert(&workload.container_name)
            {
                return Err(ManifestError::DuplicateWorkloadIdentity);
            }
        }

        let canonical = canonical::to_canonical_json(&ManifestPreimage {
            schema_version: self.schema_version,
            worker_api_range: self.worker_api_range,
            producer: &self.producer,
            host_id: self.host_id,
            target: self.target,
            manager: self.manager,
            workload_count: self.workload_count,
            workloads: &self.workloads,
        })?;
        let expected = Sha256Identity::from_computed(canonical::sha256_hex(&canonical));
        if self.digest != expected || self.generation_id != expected {
            return Err(ManifestError::ManifestDigest);
        }
        Ok(())
    }
}

impl WorkloadRecord {
    /// Returns the manifest-issued workload identity.
    #[must_use]
    pub const fn workload_id(&self) -> &Sha256Identity {
        &self.workload_id
    }

    /// Returns the workload name.
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name.0
    }

    /// Returns the workload target.
    #[must_use]
    pub const fn target(&self) -> WorkerTarget {
        self.target
    }

    /// Returns whether the workload is enabled.
    #[must_use]
    pub const fn is_enabled(&self) -> bool {
        self.enabled
    }

    /// Returns the lifecycle kind.
    #[must_use]
    pub const fn lifecycle(&self) -> WorkloadLifecycle {
        self.lifecycle
    }

    /// Returns the startup intent.
    #[must_use]
    pub const fn startup_intent(&self) -> StartupIntent {
        self.startup_intent
    }

    /// Returns the non-secret source identity.
    #[must_use]
    pub fn source_identity(&self) -> &str {
        &self.source_identity.0
    }

    /// Returns the source digest.
    #[must_use]
    pub const fn source_digest(&self) -> &Sha256Identity {
        &self.source_digest
    }

    /// Returns the resolved-intent digest.
    #[must_use]
    pub const fn resolved_digest(&self) -> &Sha256Identity {
        &self.resolved_digest
    }

    /// Returns the dependency-graph digest.
    #[must_use]
    pub const fn dependency_digest(&self) -> &Sha256Identity {
        &self.dependency_digest
    }

    /// Returns the Quadlet source-unit name.
    #[must_use]
    pub fn quadlet_source_unit(&self) -> &str {
        &self.quadlet_source_unit.0
    }

    /// Returns the expected generated-service name.
    #[must_use]
    pub fn generated_service(&self) -> &str {
        &self.generated_service.0
    }

    /// Returns the expected runtime container name.
    #[must_use]
    pub fn container_name(&self) -> &str {
        &self.container_name.0
    }

    /// Returns the materialized artifact identity.
    #[must_use]
    pub const fn artifact_identity(&self) -> &Sha256Identity {
        &self.artifact_identity
    }

    /// Returns the root filesystem Nix store path.
    #[must_use]
    pub fn rootfs_store_path(&self) -> &str {
        &self.rootfs_store_path.0
    }

    /// Returns the closure identity.
    #[must_use]
    pub const fn closure_identity(&self) -> &Sha256Identity {
        &self.closure_identity
    }

    /// Returns the ordered dependency-service identities.
    pub fn dependency_services(&self) -> impl Iterator<Item = &str> {
        self.dependency_services.iter().map(|unit| unit.0.as_str())
    }

    /// Returns the ordered supported lifecycle operations.
    #[must_use]
    pub fn lifecycle_capabilities(&self) -> &[LifecycleCapability] {
        &self.lifecycle_capabilities
    }

    /// Returns the ordered supported observability layers.
    #[must_use]
    pub fn observability_capabilities(&self) -> &[ObservabilityCapability] {
        &self.observability_capabilities
    }

    /// Returns the required worker API range.
    #[must_use]
    pub const fn required_worker_api(&self) -> ProtocolVersionRange {
        self.required_worker_api
    }

    /// Returns the required producer identity.
    #[must_use]
    pub const fn required_producer(&self) -> &ProducerIdentity {
        &self.required_producer
    }

    /// Returns the required runtime-backend compatibility.
    #[must_use]
    pub const fn required_backend(&self) -> &BackendRequirement {
        &self.required_backend
    }

    fn validate(
        &self,
        target: WorkerTarget,
        worker_api_range: ProtocolVersionRange,
        producer: &ProducerIdentity,
    ) -> Result<(), ManifestError> {
        if self.target != target {
            return Err(ManifestError::ContextMismatch);
        }
        if self.quadlet_source_unit.0.strip_suffix(".container")
            != self.generated_service.0.strip_suffix(".service")
        {
            return Err(ManifestError::WorkloadUnitMismatch);
        }
        if self.dependency_services.len() > MAX_WORKLOAD_DEPENDENCIES
            || !strictly_sorted_unique(&self.dependency_services)
            || !strictly_sorted_unique(&self.lifecycle_capabilities)
            || !strictly_sorted_unique(&self.observability_capabilities)
        {
            return Err(ManifestError::WorkloadCollection);
        }
        if &self.required_producer != producer {
            return Err(ManifestError::ProducerMismatch);
        }
        if self.required_worker_api.major() != worker_api_range.major()
            || self.required_worker_api.min_minor() < worker_api_range.min_minor()
            || self.required_worker_api.max_minor() > worker_api_range.max_minor()
            || self.required_worker_api.min_minor() > PROTOCOL_MAX_MINOR
        {
            return Err(ManifestError::ApiCompatibility);
        }
        Ok(())
    }
}

/// Typed endpoint socket address.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind", content = "value")]
pub enum EndpointAddress {
    /// Fixed absolute system-worker socket.
    AbsoluteSystem(AbsoluteSystemEndpoint),
    /// Fixed suffix resolved below `/run/user/<effective-uid>`.
    LinuxUserRuntimeRelative(UserRuntimeEndpoint),
}

/// Exact system-worker socket path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AbsoluteSystemEndpoint;

impl<'de> Deserialize<'de> for AbsoluteSystemEndpoint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value != "/run/graft/system/worker.sock" {
            return Err(serde::de::Error::custom("invalid system endpoint path"));
        }
        Ok(Self)
    }
}

impl Serialize for UserRuntimeEndpoint {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("graft/user/worker.sock")
    }
}

impl Serialize for AbsoluteSystemEndpoint {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str("/run/graft/system/worker.sock")
    }
}

/// Exact user-runtime-relative worker socket suffix.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UserRuntimeEndpoint;

impl<'de> Deserialize<'de> for UserRuntimeEndpoint {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value != "graft/user/worker.sock" {
            return Err(serde::de::Error::custom("invalid user endpoint suffix"));
        }
        Ok(Self)
    }
}

/// Validated endpoint descriptor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EndpointDescriptor {
    schema_version: ManifestSchemaVersion,
    worker_api_range: ProtocolVersionRange,
    producer: ProducerIdentity,
    host_id: HostIdentifier,
    target: WorkerTarget,
    manager: ManagerKind,
    generation_id: Sha256Identity,
    manifest_digest: Sha256Identity,
    socket_address: EndpointAddress,
    endpoint_digest: Sha256Identity,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct EndpointPreimage<'a> {
    schema_version: ManifestSchemaVersion,
    worker_api_range: ProtocolVersionRange,
    producer: &'a ProducerIdentity,
    host_id: HostIdentifier,
    target: WorkerTarget,
    manager: ManagerKind,
    generation_id: &'a Sha256Identity,
    manifest_digest: &'a Sha256Identity,
    socket_address: &'a EndpointAddress,
}

impl EndpointDescriptor {
    /// Parses and fully validates one endpoint descriptor.
    ///
    /// # Errors
    ///
    /// Returns an error for malformed JSON, incompatible schema/context/address,
    /// or endpoint digest mismatch.
    pub fn from_json(bytes: &[u8]) -> Result<Self, ManifestError> {
        if u64::try_from(bytes.len()).map_or(true, |length| length > super::MAX_ENDPOINT_BYTES) {
            return Err(ManifestError::DocumentTooLarge);
        }
        let endpoint: Self = serde_json::from_slice(bytes).map_err(ManifestError::EndpointJson)?;
        endpoint.validate()?;
        if canonical::to_canonical_json(&endpoint)? != bytes {
            return Err(ManifestError::NonCanonicalJson);
        }
        Ok(endpoint)
    }

    /// Returns the compatible worker API range.
    #[must_use]
    pub const fn api_range(&self) -> ProtocolVersionRange {
        self.worker_api_range
    }

    /// Returns the producer identity.
    #[must_use]
    pub const fn producer(&self) -> &ProducerIdentity {
        &self.producer
    }

    /// Returns the Nix-issued host identity.
    #[must_use]
    pub const fn host_id(&self) -> HostIdentifier {
        self.host_id
    }

    /// Returns the endpoint target.
    #[must_use]
    pub const fn target(&self) -> WorkerTarget {
        self.target
    }

    /// Returns the endpoint manager kind.
    #[must_use]
    pub const fn manager(&self) -> ManagerKind {
        self.manager
    }

    /// Returns the advertised manifest generation.
    #[must_use]
    pub const fn generation_id(&self) -> &Sha256Identity {
        &self.generation_id
    }

    /// Returns the advertised manifest digest.
    #[must_use]
    pub const fn manifest_digest(&self) -> &Sha256Identity {
        &self.manifest_digest
    }

    /// Returns the typed socket address.
    #[must_use]
    pub const fn socket_address(&self) -> &EndpointAddress {
        &self.socket_address
    }

    /// Returns the validated endpoint digest.
    #[must_use]
    pub const fn endpoint_digest(&self) -> &Sha256Identity {
        &self.endpoint_digest
    }

    fn validate(&self) -> Result<(), ManifestError> {
        if !self.schema_version.is_compatible() {
            return Err(ManifestError::IncompatibleSchema);
        }
        validate_context(self.target, self.manager)?;
        validate_api_compatibility(self.worker_api_range)?;
        if self.generation_id != self.manifest_digest {
            return Err(ManifestError::DescriptorMismatch);
        }
        if !matches!(
            (self.target, &self.socket_address),
            (WorkerTarget::System, EndpointAddress::AbsoluteSystem(_))
                | (
                    WorkerTarget::User,
                    EndpointAddress::LinuxUserRuntimeRelative(_)
                )
        ) {
            return Err(ManifestError::EndpointContext);
        }
        let canonical = canonical::to_canonical_json(&EndpointPreimage {
            schema_version: self.schema_version,
            worker_api_range: self.worker_api_range,
            producer: &self.producer,
            host_id: self.host_id,
            target: self.target,
            manager: self.manager,
            generation_id: &self.generation_id,
            manifest_digest: &self.manifest_digest,
            socket_address: &self.socket_address,
        })?;
        if self.endpoint_digest != Sha256Identity::from_computed(canonical::sha256_hex(&canonical))
        {
            return Err(ManifestError::EndpointDigest);
        }
        Ok(())
    }
}

pub(super) fn validate_pair(
    manifest: &Manifest,
    endpoint: &EndpointDescriptor,
) -> Result<(), ManifestError> {
    if manifest.schema_version != endpoint.schema_version
        || manifest.api_range() != endpoint.worker_api_range
        || manifest.producer() != &endpoint.producer
        || manifest.host_id() != endpoint.host_id
        || manifest.target != endpoint.target
        || manifest.manager != endpoint.manager
        || manifest.generation_id != endpoint.generation_id
        || manifest.digest() != &endpoint.manifest_digest
    {
        return Err(ManifestError::DescriptorMismatch);
    }
    Ok(())
}

fn validate_api_compatibility(range: ProtocolVersionRange) -> Result<(), ManifestError> {
    if range.major() != PROTOCOL_MAJOR || range.min_minor() > PROTOCOL_MAX_MINOR {
        return Err(ManifestError::ApiCompatibility);
    }
    Ok(())
}

fn validate_context(target: WorkerTarget, manager: ManagerKind) -> Result<(), ManifestError> {
    if !matches!(
        (target, manager),
        (WorkerTarget::System, ManagerKind::System) | (WorkerTarget::User, ManagerKind::User)
    ) {
        return Err(ManifestError::ContextMismatch);
    }
    Ok(())
}

fn strictly_sorted_unique<T: Ord>(values: &[T]) -> bool {
    values.windows(2).all(|pair| pair[0] < pair[1])
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
struct WorkloadName(String);

impl<'de> Deserialize<'de> for WorkloadName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        let mut bytes = value.bytes();
        if value.len() > MAX_MANIFEST_STRING_BYTES
            || !bytes
                .next()
                .is_some_and(|byte| byte.is_ascii_alphanumeric())
            || !bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.'))
        {
            return Err(serde::de::Error::custom("invalid workload name"));
        }
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
struct BoundedIdentifier(String);

impl<'de> Deserialize<'de> for BoundedIdentifier {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.is_empty()
            || value.len() > MAX_MANIFEST_STRING_BYTES
            || !value.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'@')
            })
        {
            return Err(serde::de::Error::custom("invalid bounded identifier"));
        }
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
struct BoundedText(String);

impl<'de> Deserialize<'de> for BoundedText {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.is_empty()
            || value.len() > MAX_MANIFEST_STRING_BYTES
            || value.chars().any(char::is_control)
        {
            return Err(serde::de::Error::custom("invalid bounded text"));
        }
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
struct SourceUnitName(String);

impl<'de> Deserialize<'de> for SourceUnitName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = BoundedIdentifier::deserialize(deserializer)?.0;
        if !value.ends_with(".container") {
            return Err(serde::de::Error::custom(
                "source unit must end in .container",
            ));
        }
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
struct ServiceUnitName(String);

impl<'de> Deserialize<'de> for ServiceUnitName {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = BoundedIdentifier::deserialize(deserializer)?.0;
        if !value.ends_with(".service") {
            return Err(serde::de::Error::custom(
                "service unit must end in .service",
            ));
        }
        Ok(Self(value))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
struct NixStorePath(String);

impl<'de> Deserialize<'de> for NixStorePath {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        let Some(base) = value.strip_prefix("/nix/store/") else {
            return Err(serde::de::Error::custom(
                "path is not an absolute Nix store path",
            ));
        };
        if base.is_empty()
            || matches!(base, "." | "..")
            || base.contains('/')
            || base.len() > MAX_MANIFEST_STRING_BYTES
            || !base.bytes().all(|byte| {
                byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+')
            })
        {
            return Err(serde::de::Error::custom("invalid Nix store path"));
        }
        Ok(Self(value))
    }
}
