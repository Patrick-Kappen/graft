//! Deterministic construction of Nix-published discovery documents.

use serde::{Deserialize, Serialize};

use crate::protocol::{ManagerKind, ProtocolVersionRange, WorkerTarget};

use super::{schema, HostIdentifier, ManifestError, ProducerIdentity, WorkloadRecord};

/// Typed, already-resolved facts from which a discovery generation is rendered.
///
/// This deliberately contains no source configuration, filesystem paths outside
/// a materialised workload record, credentials, timestamps, or publication
/// state. The endpoint address is derived from `target` rather than supplied by
/// a caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RenderInput {
    pub(super) producer: ProducerIdentity,
    pub(super) host_id: HostIdentifier,
    pub(super) target: WorkerTarget,
    pub(super) manager: ManagerKind,
    pub(super) worker_api_range: ProtocolVersionRange,
    pub(super) workloads: Vec<WorkloadRecord>,
}

impl RenderInput {
    /// Creates a typed discovery-document preimage.
    #[must_use]
    pub fn new(
        producer: ProducerIdentity,
        host_id: HostIdentifier,
        target: WorkerTarget,
        manager: ManagerKind,
        worker_api_range: ProtocolVersionRange,
        workloads: Vec<WorkloadRecord>,
    ) -> Self {
        Self {
            producer,
            host_id,
            target,
            manager,
            worker_api_range,
            workloads,
        }
    }
}

/// Canonical immutable discovery documents and their computed identities.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenderedDocuments {
    manifest: Vec<u8>,
    endpoint: Vec<u8>,
}

impl RenderedDocuments {
    pub(super) const fn new(manifest: Vec<u8>, endpoint: Vec<u8>) -> Self {
        Self { manifest, endpoint }
    }

    /// Returns canonical `manifest.json` bytes.
    #[must_use]
    pub fn manifest_json(&self) -> &[u8] {
        &self.manifest
    }

    /// Returns canonical `endpoint.json` bytes.
    #[must_use]
    pub fn endpoint_json(&self) -> &[u8] {
        &self.endpoint
    }
}

/// Renders one canonical manifest/endpoint pair.
///
/// # Errors
/// Returns an error when the preimage violates the same bounds, ordering,
/// identity, compatibility, or context rules enforced by the consumer.
pub fn render(input: RenderInput) -> Result<RenderedDocuments, ManifestError> {
    schema::render_documents(input)
}
