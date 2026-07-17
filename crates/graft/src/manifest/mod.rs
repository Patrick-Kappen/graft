//! Typed, fail-closed loading of Nix-published worker discovery generations.

mod canonical;
mod filesystem;
mod schema;

use thiserror::Error;

pub use filesystem::{GenerationOwner, GenerationSnapshot, ManifestLoader};
pub use schema::{
    AbsoluteSystemEndpoint, BackendRequirement, EndpointAddress, EndpointDescriptor,
    HostIdentifier, LifecycleCapability, Manifest, ManifestSchemaVersion, ObservabilityCapability,
    ProducerIdentity, RuntimeBackend, Sha256Identity, StartupIntent, UserRuntimeEndpoint,
    WorkloadLifecycle, WorkloadRecord, MANIFEST_SCHEMA_MAJOR, MANIFEST_SCHEMA_MAX_MINOR,
    MANIFEST_SCHEMA_MIN_MINOR, MAX_MANIFEST_STRING_BYTES, MAX_MANIFEST_WORKLOADS,
    MAX_WORKLOAD_DEPENDENCIES,
};

/// Maximum accepted manifest document size.
pub const MAX_MANIFEST_BYTES: u64 = 1024 * 1024;
/// Maximum accepted endpoint descriptor size.
pub const MAX_ENDPOINT_BYTES: u64 = 16 * 1024;

/// Fail-closed discovery generation error.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ManifestError {
    /// A filesystem operation failed without exposing untrusted file content.
    #[error("discovery generation filesystem validation failed")]
    Filesystem(#[source] std::io::Error),
    /// A path component has an unexpected file type.
    #[error("discovery generation contains an unexpected file type")]
    FileType,
    /// A path has unexpected ownership.
    #[error("discovery generation ownership does not match worker policy")]
    Ownership,
    /// A path has unexpected permissions.
    #[error("discovery generation permissions do not match worker policy")]
    Permissions,
    /// A generation contains an unexpected directory entry.
    #[error("discovery generation contains unexpected entries")]
    UnexpectedEntry,
    /// The configured pointer does not directly reference a Nix store generation.
    #[error("current pointer does not directly reference a Nix store generation")]
    GenerationReference,
    /// A document exceeds its directional limit.
    #[error("discovery document exceeds its size limit")]
    DocumentTooLarge,
    /// The manifest JSON is malformed or violates its typed schema.
    #[error("manifest JSON is invalid")]
    ManifestJson(#[source] serde_json::Error),
    /// The endpoint JSON is malformed or violates its typed schema.
    #[error("endpoint JSON is invalid")]
    EndpointJson(#[source] serde_json::Error),
    /// Canonical serialization failed.
    #[error("canonical JSON serialization failed")]
    Serialize(#[source] serde_json::Error),
    /// A floating-point value is not part of canonical manifest JSON.
    #[error("canonical manifest JSON forbids floating-point values")]
    FloatingPoint,
    /// Input bytes are not the unique canonical representation.
    #[error("discovery document is not canonical JSON")]
    NonCanonicalJson,
    /// The manifest schema is unsupported.
    #[error("manifest schema version is incompatible")]
    IncompatibleSchema,
    /// Target and manager context are inconsistent.
    #[error("manifest target and manager context do not match")]
    ContextMismatch,
    /// The endpoint address does not match its context.
    #[error("endpoint address does not match its context")]
    EndpointContext,
    /// The workload count is invalid.
    #[error("manifest workload count is invalid")]
    WorkloadCount,
    /// Workloads are not strictly sorted by name.
    #[error("manifest workloads are not in canonical order")]
    WorkloadOrder,
    /// A workload identity is duplicated.
    #[error("manifest contains duplicate workload identity")]
    DuplicateWorkloadIdentity,
    /// A bounded workload collection is invalid.
    #[error("manifest workload collection is unsorted, duplicated, or oversized")]
    WorkloadCollection,
    /// Workload producer requirements do not match the envelope.
    #[error("workload producer requirement does not match manifest producer")]
    ProducerMismatch,
    /// Workload API requirements exceed the envelope.
    #[error("workload API requirement exceeds manifest compatibility")]
    ApiCompatibility,
    /// The manifest generation or digest is incorrect.
    #[error("manifest digest or generation identity is invalid")]
    ManifestDigest,
    /// The endpoint digest is incorrect.
    #[error("endpoint descriptor digest is invalid")]
    EndpointDigest,
    /// Manifest and endpoint descriptor identities differ.
    #[error("manifest and endpoint descriptor do not describe one generation")]
    DescriptorMismatch,
}
