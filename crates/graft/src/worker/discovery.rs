//! Manifest-bound discovery and read-only status dispatcher.

#[cfg(feature = "worker-test-fixtures")]
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt::Debug;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use sha2::{Digest as _, Sha256};
use uuid::Uuid;

use crate::manifest::{ManifestError, ManifestLoader, WorkloadRecord};
use crate::protocol::{
    encoded_frame_len, FrameDirection, ManifestGeneration, WorkerTarget, MAX_OUTBOUND_FRAME_BYTES,
};

use super::dispatcher::{DispatchContext, DispatchFailure, DispatchPlan, SemanticDispatcher};
use super::observation::{
    derive_summary, Availability, DeclaredLayer, GeneratedState, InspectSnapshot,
    ListStatusRequest, ManagerState, MaterialisedState, ObservationText, ObservedLayer, PageCursor,
    ResolvedLayer, RuntimeObservation, StatusFilter, StatusPage, StatusSummary, SummaryEvidence,
    WorkloadSelector, WorkloadSnapshot, MAX_STATUS_PAGE_SIZE,
};
use super::protocol::{ReadOnlyResponse, SemanticRequest, WorkerErrorCode};

const CURSOR_TTL: Duration = Duration::from_secs(60);
const MAX_RETAINED_CURSORS: usize = 1_024;

/// Worker-owned backend selector derived only from a validated manifest record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendSelector {
    /// Manifest workload name used only for deterministic adapter lookup.
    pub workload_name: ObservationText,
    /// Expected generated service identity.
    pub generated_service: ObservationText,
    /// Expected container identity.
    pub container_name: ObservationText,
}

/// Typed generated-unit and manager layers without raw D-Bus values.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerLayers {
    /// Generated-unit evidence.
    pub generated: Availability<GeneratedState>,
    /// Manager evidence.
    pub manager: Availability<ManagerState>,
}

/// Narrow adapter for allowlisted systemd status.
pub trait ManagerStatusAdapter: Send + Sync + Debug {
    /// Observes one manifest-bound unit without accepting a client unit name.
    fn observe(&self, selector: &BackendSelector) -> ManagerLayers;
    /// Whether controlled manager observation is implemented.
    fn is_supported(&self) -> bool;
}

/// Narrow adapter for allowlisted Podman status.
pub trait RuntimeStatusAdapter: Send + Sync + Debug {
    /// Observes one manifest-bound container without accepting a client container name.
    fn observe(&self, selector: &BackendSelector) -> Availability<RuntimeObservation>;
    /// Whether controlled runtime observation is implemented.
    fn is_supported(&self) -> bool;
}

/// Honest manager fallback until controlled D-Bus bindings are installed.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableManagerAdapter;

impl ManagerStatusAdapter for UnavailableManagerAdapter {
    fn observe(&self, _selector: &BackendSelector) -> ManagerLayers {
        ManagerLayers {
            generated: Availability::Unsupported,
            manager: Availability::Unavailable,
        }
    }

    fn is_supported(&self) -> bool {
        false
    }
}

/// Honest runtime fallback until controlled Podman bindings are installed.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnavailableRuntimeAdapter;

impl RuntimeStatusAdapter for UnavailableRuntimeAdapter {
    fn observe(&self, _selector: &BackendSelector) -> Availability<RuntimeObservation> {
        Availability::Unavailable
    }

    fn is_supported(&self) -> bool {
        false
    }
}

/// Deterministic typed systemd adapter for status matrix tests.
#[cfg(feature = "worker-test-fixtures")]
#[derive(Debug, Clone)]
pub struct MockManagerAdapter {
    observations: BTreeMap<String, ManagerLayers>,
    fallback: ManagerLayers,
}

#[cfg(feature = "worker-test-fixtures")]
impl MockManagerAdapter {
    /// Creates a mock with per-workload manager observations and a fallback.
    #[must_use]
    pub fn new(observations: BTreeMap<String, ManagerLayers>, fallback: ManagerLayers) -> Self {
        Self {
            observations,
            fallback,
        }
    }
}

#[cfg(feature = "worker-test-fixtures")]
impl ManagerStatusAdapter for MockManagerAdapter {
    fn observe(&self, selector: &BackendSelector) -> ManagerLayers {
        self.observations
            .get(selector.workload_name.as_str())
            .unwrap_or(&self.fallback)
            .clone()
    }

    fn is_supported(&self) -> bool {
        true
    }
}

/// Deterministic typed Podman adapter for status matrix tests.
#[cfg(feature = "worker-test-fixtures")]
#[derive(Debug, Clone)]
pub struct MockRuntimeAdapter {
    observations: BTreeMap<String, Availability<RuntimeObservation>>,
    fallback: Availability<RuntimeObservation>,
}

#[cfg(feature = "worker-test-fixtures")]
impl MockRuntimeAdapter {
    /// Creates a mock with per-workload runtime observations and a fallback.
    #[must_use]
    pub fn new(
        observations: BTreeMap<String, Availability<RuntimeObservation>>,
        fallback: Availability<RuntimeObservation>,
    ) -> Self {
        Self {
            observations,
            fallback,
        }
    }
}

#[cfg(feature = "worker-test-fixtures")]
impl RuntimeStatusAdapter for MockRuntimeAdapter {
    fn observe(&self, selector: &BackendSelector) -> Availability<RuntimeObservation> {
        self.observations
            .get(selector.workload_name.as_str())
            .unwrap_or(&self.fallback)
            .clone()
    }

    fn is_supported(&self) -> bool {
        true
    }
}

#[derive(Debug, Clone)]
struct CursorBinding {
    worker_epoch: crate::protocol::ConnectionIdentifier,
    uid: u32,
    authorization_revision: u64,
    generation: ManifestGeneration,
    filter: Option<StatusFilter>,
    membership: [u8; 32],
    revision: u64,
    offset: usize,
    expires_at: Instant,
}

#[derive(Debug, Default)]
struct RevisionState {
    revision: u64,
    membership: Option<[u8; 32]>,
}

#[derive(Debug, Default)]
struct PaginationState {
    revisions: HashMap<String, RevisionState>,
    cursors: HashMap<String, CursorBinding>,
}

/// User-scope dispatcher backed by coherent manifest generations.
#[derive(Debug, Clone)]
pub struct DiscoveryDispatcher {
    loader: ManifestLoader,
    effective_uid: u32,
    manager: Arc<dyn ManagerStatusAdapter>,
    runtime: Arc<dyn RuntimeStatusAdapter>,
    authorization_revision: Arc<AtomicU64>,
    pagination: Arc<Mutex<PaginationState>>,
}

impl DiscoveryDispatcher {
    /// Creates a dispatcher fixed to one user and manifest loader.
    #[must_use]
    pub fn new(
        loader: ManifestLoader,
        effective_uid: u32,
        manager: Arc<dyn ManagerStatusAdapter>,
        runtime: Arc<dyn RuntimeStatusAdapter>,
    ) -> Self {
        Self {
            loader,
            effective_uid,
            manager,
            runtime,
            authorization_revision: Arc::new(AtomicU64::new(1)),
            pagination: Arc::new(Mutex::new(PaginationState::default())),
        }
    }

    #[cfg(test)]
    fn set_authorization_revision(&self, revision: u64) {
        self.authorization_revision
            .store(revision, Ordering::Release);
    }

    fn authorize(&self, context: &DispatchContext) -> Result<(), DispatchFailure> {
        if context.principal.target != WorkerTarget::User
            || context.principal.uid != self.effective_uid
            || context.peer.uid != self.effective_uid
        {
            return Err(DispatchFailure::new(
                WorkerErrorCode::Unauthorized,
                "request is not authorized for this worker context",
            ));
        }
        Ok(())
    }

    fn snapshots(
        &self,
        worker_epoch: crate::protocol::ConnectionIdentifier,
    ) -> Result<(ManifestGeneration, Vec<WorkloadSnapshot>), DispatchFailure> {
        let generation = self.loader.load().map_err(|error| {
            if matches!(error, ManifestError::ContextMismatch) {
                DispatchFailure::new(
                    WorkerErrorCode::Unauthorized,
                    "request is not authorized for this worker context",
                )
            } else {
                DispatchFailure::new(
                    WorkerErrorCode::ManifestUnavailable,
                    "current manifest is unavailable",
                )
            }
        })?;
        let manifest = generation.manifest();
        if manifest.target() != WorkerTarget::User {
            return Err(DispatchFailure::new(
                WorkerErrorCode::Unauthorized,
                "request is not authorized for this worker context",
            ));
        }
        let generation_id =
            ManifestGeneration::parse(manifest.generation_id().as_str()).map_err(|_| {
                DispatchFailure::new(
                    WorkerErrorCode::ManifestUnavailable,
                    "current manifest is invalid",
                )
            })?;
        let snapshots = manifest
            .workloads()
            .iter()
            .map(|workload| self.snapshot(worker_epoch, &generation_id, workload))
            .collect::<Result<Vec<_>, _>>()?;
        Ok((generation_id, snapshots))
    }

    #[allow(clippy::too_many_lines)]
    fn snapshot(
        &self,
        worker_epoch: crate::protocol::ConnectionIdentifier,
        generation: &ManifestGeneration,
        workload: &WorkloadRecord,
    ) -> Result<WorkloadSnapshot, DispatchFailure> {
        let backend_selector = BackendSelector {
            workload_name: ObservationText::parse(workload.name()).map_err(|_| {
                DispatchFailure::new(
                    WorkerErrorCode::ManifestUnavailable,
                    "manifest workload identity is invalid",
                )
            })?,
            generated_service: ObservationText::parse(workload.generated_service()).map_err(
                |_| {
                    DispatchFailure::new(
                        WorkerErrorCode::ManifestUnavailable,
                        "manifest service identity is invalid",
                    )
                },
            )?,
            container_name: ObservationText::parse(workload.container_name()).map_err(|_| {
                DispatchFailure::new(
                    WorkerErrorCode::ManifestUnavailable,
                    "manifest container identity is invalid",
                )
            })?,
        };
        let manager = self.manager.observe(&backend_selector);
        let runtime = self.runtime.observe(&backend_selector);
        let materialised = Availability::Fresh(MaterialisedState {
            artifacts_present: true,
            valid: true,
            artifact_identity: ManifestGeneration::parse(workload.artifact_identity().as_str())
                .map_err(|_| {
                    DispatchFailure::new(
                        WorkerErrorCode::ManifestUnavailable,
                        "manifest artifact identity is invalid",
                    )
                })?,
            closure_identity: ManifestGeneration::parse(workload.closure_identity().as_str())
                .map_err(|_| {
                    DispatchFailure::new(
                        WorkerErrorCode::ManifestUnavailable,
                        "manifest closure identity is invalid",
                    )
                })?,
        });
        let summary = derive_summary(&SummaryEvidence {
            enabled: workload.is_enabled(),
            lifecycle: workload.lifecycle(),
            materialised: materialised.clone(),
            generated: manager.generated.clone(),
            manager: manager.manager.clone(),
            runtime: runtime.clone(),
        });
        let runtime_matches = !matches!(
            &runtime,
            Availability::Fresh(RuntimeObservation {
                identity_matches: false,
                ..
            })
        );
        Ok(WorkloadSnapshot {
            worker_epoch,
            effective_uid: self.effective_uid,
            authorization_revision: self.authorization_revision.load(Ordering::Acquire),
            selector: WorkloadSelector {
                target: workload.target(),
                name: backend_selector.workload_name,
                generation: generation.clone(),
                workload_id: ManifestGeneration::parse(workload.workload_id().as_str()).map_err(
                    |_| {
                        DispatchFailure::new(
                            WorkerErrorCode::ManifestUnavailable,
                            "manifest workload identity is invalid",
                        )
                    },
                )?,
            },
            summary,
            declared: Availability::Fresh(DeclaredLayer {
                enabled: workload.is_enabled(),
                lifecycle: workload.lifecycle(),
                startup_intent: workload.startup_intent(),
                source_identity: ObservationText::parse(workload.source_identity()).map_err(
                    |_| {
                        DispatchFailure::new(
                            WorkerErrorCode::ManifestUnavailable,
                            "manifest source identity is invalid",
                        )
                    },
                )?,
                source_digest: ManifestGeneration::parse(workload.source_digest().as_str())
                    .map_err(|_| {
                        DispatchFailure::new(
                            WorkerErrorCode::ManifestUnavailable,
                            "manifest source digest is invalid",
                        )
                    })?,
                lifecycle_capabilities: workload.lifecycle_capabilities().to_vec(),
                observability_capabilities: workload.observability_capabilities().to_vec(),
            }),
            resolved: Availability::Fresh(ResolvedLayer {
                resolved_digest: ManifestGeneration::parse(workload.resolved_digest().as_str())
                    .map_err(|_| {
                        DispatchFailure::new(
                            WorkerErrorCode::ManifestUnavailable,
                            "manifest resolver identity is invalid",
                        )
                    })?,
                dependency_digest: ManifestGeneration::parse(workload.dependency_digest().as_str())
                    .map_err(|_| {
                        DispatchFailure::new(
                            WorkerErrorCode::ManifestUnavailable,
                            "manifest dependency identity is invalid",
                        )
                    })?,
                producer_version: ObservationText::parse(workload.required_producer().version())
                    .map_err(|_| {
                        DispatchFailure::new(
                            WorkerErrorCode::ManifestUnavailable,
                            "manifest producer version is invalid",
                        )
                    })?,
            }),
            materialised,
            generated: manager.generated,
            manager: manager.manager,
            runtime,
            observed: Availability::Fresh(ObservedLayer {
                identity_matches: runtime_matches,
            }),
        })
    }

    fn find_snapshot(
        generation: &ManifestGeneration,
        snapshots: Vec<WorkloadSnapshot>,
        selector: &WorkloadSelector,
    ) -> Result<WorkloadSnapshot, DispatchFailure> {
        if selector.target != WorkerTarget::User {
            return Err(DispatchFailure::new(
                WorkerErrorCode::Unauthorized,
                "request is not authorized for this worker context",
            ));
        }
        if &selector.generation != generation {
            return Err(DispatchFailure::new(
                WorkerErrorCode::StaleManifest,
                "request targets a stale manifest generation",
            ));
        }
        snapshots
            .into_iter()
            .find(|snapshot| {
                snapshot.selector.name == selector.name
                    && snapshot.selector.workload_id == selector.workload_id
            })
            .ok_or_else(|| {
                DispatchFailure::new(
                    WorkerErrorCode::WorkloadNotFound,
                    "workload is not visible in the current context",
                )
            })
    }

    #[allow(clippy::too_many_lines)]
    fn list(
        &self,
        worker_epoch: crate::protocol::ConnectionIdentifier,
        uid: u32,
        request: &ListStatusRequest,
        generation: ManifestGeneration,
        snapshots: Vec<WorkloadSnapshot>,
    ) -> Result<StatusPage, DispatchFailure> {
        if request.page_size == 0 || request.page_size > MAX_STATUS_PAGE_SIZE {
            return Err(DispatchFailure::new(
                WorkerErrorCode::InvalidRequest,
                "status page size is outside the supported range",
            ));
        }
        let mut summaries = snapshots
            .into_iter()
            .map(|snapshot| StatusSummary {
                lifecycle: match snapshot.declared {
                    Availability::Fresh(ref declared) => declared.lifecycle,
                    _ => unreachable!("manifest-derived declared layer is always fresh"),
                },
                selector: snapshot.selector,
                summary: snapshot.summary,
            })
            .filter(|summary| filter_matches(summary, request.filter))
            .collect::<Vec<_>>();
        summaries.sort_by(|left, right| {
            left.selector
                .name
                .as_str()
                .cmp(right.selector.name.as_str())
        });
        let membership = membership_digest(&summaries);
        let mut state = self.pagination.lock().map_err(|_| {
            DispatchFailure::new(
                WorkerErrorCode::Unavailable,
                "pagination state is unavailable",
            )
        })?;
        let now = Instant::now();
        state.cursors.retain(|_, cursor| cursor.expires_at > now);
        let authorization_revision = self.authorization_revision.load(Ordering::Acquire);
        let revision_key = revision_key(uid, authorization_revision, request.filter);
        let revision = {
            let revision_state = state.revisions.entry(revision_key).or_default();
            if revision_state.membership != Some(membership) {
                revision_state.revision = revision_state.revision.saturating_add(1).max(1);
                revision_state.membership = Some(membership);
            }
            revision_state.revision
        };
        let offset = if let Some(cursor) = &request.cursor {
            let binding = state
                .cursors
                .get(&cursor.0)
                .cloned()
                .ok_or_else(cursor_expired)?;
            if binding.worker_epoch != worker_epoch
                || binding.uid != uid
                || binding.authorization_revision != authorization_revision
                || binding.generation != generation
                || binding.filter != request.filter
                || binding.membership != membership
                || binding.revision != revision
                || binding.expires_at <= now
            {
                return Err(cursor_expired());
            }
            binding.offset
        } else {
            0
        };
        if offset > summaries.len() {
            return Err(cursor_expired());
        }
        let end = bounded_page_end(&summaries, offset, request.page_size)?;
        let workloads = summaries[offset..end].to_vec();
        let next_cursor = if end < summaries.len() {
            if state.cursors.len() >= MAX_RETAINED_CURSORS {
                return Err(DispatchFailure::new(
                    WorkerErrorCode::Overloaded,
                    "pagination cursor capacity is exhausted",
                ));
            }
            let token = Uuid::now_v7().hyphenated().to_string();
            state.cursors.insert(
                token.clone(),
                CursorBinding {
                    worker_epoch,
                    uid,
                    authorization_revision,
                    generation,
                    filter: request.filter,
                    membership,
                    revision,
                    offset: end,
                    expires_at: now + CURSOR_TTL,
                },
            );
            Some(PageCursor(token))
        } else {
            None
        };
        Ok(StatusPage {
            worker_epoch,
            authorization_revision,
            revision,
            workloads,
            next_cursor,
        })
    }

    fn execute(
        &self,
        context: &DispatchContext,
        request: &SemanticRequest,
    ) -> Result<ReadOnlyResponse, DispatchFailure> {
        self.authorize(context)?;
        if !matches!(
            request,
            SemanticRequest::ListStatus(_)
                | SemanticRequest::GetStatus { .. }
                | SemanticRequest::Inspect { .. }
        ) {
            return Err(DispatchFailure::new(
                WorkerErrorCode::Unsupported,
                "semantic operation is unavailable",
            ));
        }
        let (generation, snapshots) = self.snapshots(context.worker_epoch)?;
        match request {
            SemanticRequest::ListStatus(request) => Ok(ReadOnlyResponse::StatusPage(self.list(
                context.worker_epoch,
                context.principal.uid,
                request,
                generation,
                snapshots,
            )?)),
            SemanticRequest::GetStatus { selector } => Ok(ReadOnlyResponse::Status(
                Self::find_snapshot(&generation, snapshots, selector)?,
            )),
            SemanticRequest::Inspect { selector } => {
                Ok(ReadOnlyResponse::Inspect(InspectSnapshot {
                    snapshot: Self::find_snapshot(&generation, snapshots, selector)?,
                    manager_supported: self.manager.is_supported(),
                    runtime_supported: self.runtime.is_supported(),
                }))
            }
            SemanticRequest::Lifecycle(_)
            | SemanticRequest::QueryLifecycle(_)
            | SemanticRequest::Reserved => Err(DispatchFailure::new(
                WorkerErrorCode::Unsupported,
                "semantic operation is unavailable",
            )),
            #[cfg(feature = "worker-test-fixtures")]
            SemanticRequest::MockUnary { .. }
            | SemanticRequest::MockLifecycle { .. }
            | SemanticRequest::MockStream { .. } => Err(DispatchFailure::new(
                WorkerErrorCode::Unsupported,
                "semantic operation is unavailable",
            )),
        }
    }
}

impl SemanticDispatcher for DiscoveryDispatcher {
    fn dispatch<'a>(
        &'a self,
        context: &'a DispatchContext,
        request: &'a SemanticRequest,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = DispatchPlan> + Send + 'a>> {
        let dispatcher = self.clone();
        let context = *context;
        let request = request.clone();
        Box::pin(async move {
            let result =
                tokio::task::spawn_blocking(move || dispatcher.execute(&context, &request))
                    .await
                    .unwrap_or_else(|_| {
                        Err(DispatchFailure::new(
                            WorkerErrorCode::Unavailable,
                            "read-only dispatcher task failed",
                        ))
                    });
            DispatchPlan::Unary(Box::new(result))
        })
    }
}

fn bounded_page_end(
    summaries: &[StatusSummary],
    offset: usize,
    page_size: u16,
) -> Result<usize, DispatchFailure> {
    let maximum_end = offset
        .saturating_add(usize::from(page_size))
        .min(summaries.len());
    let mut end = offset;
    let mut encoded_bytes = 0_usize;
    let page_budget = MAX_OUTBOUND_FRAME_BYTES.saturating_sub(4 * 1_024);
    while end < maximum_end {
        let item_bytes = encoded_frame_len(&summaries[end], FrameDirection::ServerToClient)
            .map_err(|_| {
                DispatchFailure::new(
                    WorkerErrorCode::Unavailable,
                    "status summary cannot be encoded safely",
                )
            })?;
        let candidate = encoded_bytes.saturating_add(item_bytes);
        if end > offset && candidate > page_budget {
            break;
        }
        encoded_bytes = candidate;
        end += 1;
    }
    Ok(end)
}

fn filter_matches(summary: &StatusSummary, filter: Option<StatusFilter>) -> bool {
    filter.map_or(true, |filter| {
        filter
            .lifecycle
            .map_or(true, |value| value == summary.lifecycle)
            && filter
                .summary
                .map_or(true, |value| value == summary.summary)
    })
}

fn revision_key(uid: u32, authorization_revision: u64, filter: Option<StatusFilter>) -> String {
    format!("{uid}:{authorization_revision}:{filter:?}")
}

fn membership_digest(summaries: &[StatusSummary]) -> [u8; 32] {
    let mut digest = Sha256::new();
    for summary in summaries {
        digest.update(summary.selector.name.as_str().as_bytes());
        digest.update([0]);
        digest.update(summary.selector.generation.as_str().as_bytes());
        digest.update([0]);
        digest.update(summary.selector.workload_id.as_str().as_bytes());
        digest.update([0]);
        digest.update(format!("{:?}:{:?}", summary.lifecycle, summary.summary).as_bytes());
        digest.update([0]);
    }
    digest.finalize().into()
}

fn cursor_expired() -> DispatchFailure {
    DispatchFailure::new(
        WorkerErrorCode::PageCursorExpired,
        "status page cursor expired",
    )
}

#[cfg(test)]
mod tests {
    use tempfile::TempDir;

    use crate::manifest::{
        ProducerIdentity, StartupIntent, WorkloadLifecycle, MAX_MANIFEST_STRING_BYTES,
    };
    use crate::protocol::{ConnectionIdentifier, RequestIdentifier};

    use super::*;
    use crate::worker::observation::{DeclaredLayer, Summary};

    fn dispatcher(temporary: &TempDir) -> DiscoveryDispatcher {
        DiscoveryDispatcher::new(
            ManifestLoader::user(
                temporary.path(),
                1000,
                1000,
                ProducerIdentity::new("graft", "test", "build").unwrap(),
            )
            .unwrap(),
            1000,
            Arc::new(UnavailableManagerAdapter),
            Arc::new(UnavailableRuntimeAdapter),
        )
    }

    fn worker_epoch() -> ConnectionIdentifier {
        ConnectionIdentifier::parse("018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20").unwrap()
    }

    fn generation(value: char) -> ManifestGeneration {
        ManifestGeneration::parse(value.to_string().repeat(64)).unwrap()
    }

    fn snapshot(name: &str, manifest_generation: &ManifestGeneration) -> WorkloadSnapshot {
        WorkloadSnapshot {
            worker_epoch: worker_epoch(),
            effective_uid: 1000,
            authorization_revision: 1,
            selector: WorkloadSelector {
                target: WorkerTarget::User,
                name: ObservationText::parse(name).unwrap(),
                generation: manifest_generation.clone(),
                workload_id: generation('b'),
            },
            summary: Summary::Inactive,
            declared: Availability::Fresh(DeclaredLayer {
                enabled: true,
                lifecycle: WorkloadLifecycle::LongRunning,
                startup_intent: StartupIntent::Disabled,
                source_identity: ObservationText::parse("alpha.toml").unwrap(),
                source_digest: generation('c'),
                lifecycle_capabilities: Vec::new(),
                observability_capabilities: Vec::new(),
            }),
            resolved: Availability::Unsupported,
            materialised: Availability::Unsupported,
            generated: Availability::Unsupported,
            manager: Availability::Unsupported,
            runtime: Availability::Unsupported,
            observed: Availability::Unsupported,
        }
    }

    fn first_page(
        dispatcher: &DiscoveryDispatcher,
        generation: &ManifestGeneration,
        uid: u32,
        names: &[&str],
    ) -> StatusPage {
        dispatcher
            .list(
                worker_epoch(),
                uid,
                &ListStatusRequest {
                    page_size: 1,
                    cursor: None,
                    filter: None,
                },
                generation.clone(),
                names
                    .iter()
                    .map(|name| snapshot(name, generation))
                    .collect(),
            )
            .unwrap()
    }

    #[test]
    fn authorization_denies_cross_user_and_system_context_without_lookup() {
        let temporary = TempDir::new().unwrap();
        let dispatcher = dispatcher(&temporary);
        let context = |target, principal_uid, peer_uid| DispatchContext {
            principal: super::super::dispatcher::PrincipalKey {
                target,
                uid: principal_uid,
            },
            peer: super::super::dispatcher::PeerCredentials {
                pid: 1,
                uid: peer_uid,
                gid: 1000,
            },
            worker_epoch: ConnectionIdentifier::parse("018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20")
                .unwrap(),
            connection_id: ConnectionIdentifier::parse("018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c21")
                .unwrap(),
            request_id: RequestIdentifier::new(1).unwrap(),
        };

        assert_eq!(
            dispatcher
                .authorize(&context(WorkerTarget::User, 1001, 1001))
                .unwrap_err()
                .code,
            WorkerErrorCode::Unauthorized
        );
        assert_eq!(
            dispatcher
                .authorize(&context(WorkerTarget::System, 1000, 1000))
                .unwrap_err()
                .code,
            WorkerErrorCode::Unauthorized
        );
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn pagination_expires_on_principal_generation_filter_and_membership_changes() {
        let temporary = TempDir::new().unwrap();
        let dispatcher = dispatcher(&temporary);
        let current = generation('a');
        let names = ["alpha", "beta"];

        let page = first_page(&dispatcher, &current, 1000, &names);
        let cursor = page.next_cursor.unwrap();
        {
            let mut pagination = dispatcher.pagination.lock().unwrap();
            pagination.cursors.get_mut(&cursor.0).unwrap().expires_at = Instant::now();
        }
        let expired = dispatcher.list(
            worker_epoch(),
            1000,
            &ListStatusRequest {
                page_size: 1,
                cursor: Some(cursor),
                filter: None,
            },
            current.clone(),
            names.iter().map(|name| snapshot(name, &current)).collect(),
        );
        assert_eq!(
            expired.unwrap_err().code,
            WorkerErrorCode::PageCursorExpired
        );

        let cursor = first_page(&dispatcher, &current, 1000, &names)
            .next_cursor
            .unwrap();
        let wrong_uid = dispatcher.list(
            worker_epoch(),
            1001,
            &ListStatusRequest {
                page_size: 1,
                cursor: Some(cursor),
                filter: None,
            },
            current.clone(),
            names.iter().map(|name| snapshot(name, &current)).collect(),
        );
        assert_eq!(
            wrong_uid.unwrap_err().code,
            WorkerErrorCode::PageCursorExpired
        );

        let cursor = first_page(&dispatcher, &current, 1000, &names)
            .next_cursor
            .unwrap();
        let wrong_generation = dispatcher.list(
            worker_epoch(),
            1000,
            &ListStatusRequest {
                page_size: 1,
                cursor: Some(cursor),
                filter: None,
            },
            generation('c'),
            names.iter().map(|name| snapshot(name, &current)).collect(),
        );
        assert_eq!(
            wrong_generation.unwrap_err().code,
            WorkerErrorCode::PageCursorExpired
        );

        let cursor = first_page(&dispatcher, &current, 1000, &names)
            .next_cursor
            .unwrap();
        let wrong_filter = dispatcher.list(
            worker_epoch(),
            1000,
            &ListStatusRequest {
                page_size: 1,
                cursor: Some(cursor),
                filter: Some(StatusFilter {
                    lifecycle: None,
                    summary: Some(Summary::Inactive),
                }),
            },
            current.clone(),
            names.iter().map(|name| snapshot(name, &current)).collect(),
        );
        assert_eq!(
            wrong_filter.unwrap_err().code,
            WorkerErrorCode::PageCursorExpired
        );

        let cursor = first_page(&dispatcher, &current, 1000, &names)
            .next_cursor
            .unwrap();
        dispatcher.set_authorization_revision(2);
        let changed_authorization = dispatcher.list(
            worker_epoch(),
            1000,
            &ListStatusRequest {
                page_size: 1,
                cursor: Some(cursor),
                filter: None,
            },
            current.clone(),
            names.iter().map(|name| snapshot(name, &current)).collect(),
        );
        assert_eq!(
            changed_authorization.unwrap_err().code,
            WorkerErrorCode::PageCursorExpired
        );

        let cursor = first_page(&dispatcher, &current, 1000, &names)
            .next_cursor
            .unwrap();
        let changed_membership = dispatcher.list(
            worker_epoch(),
            1000,
            &ListStatusRequest {
                page_size: 1,
                cursor: Some(cursor),
                filter: None,
            },
            current.clone(),
            ["alpha", "gamma"]
                .iter()
                .map(|name| snapshot(name, &current))
                .collect(),
        );
        assert_eq!(
            changed_membership.unwrap_err().code,
            WorkerErrorCode::PageCursorExpired
        );
    }

    #[test]
    fn status_page_stops_before_the_outbound_frame_budget() {
        let temporary = TempDir::new().unwrap();
        let dispatcher = dispatcher(&temporary);
        let current = generation('a');
        let names = (0..100)
            .map(|index| format!("{index:03}{}", "a".repeat(MAX_MANIFEST_STRING_BYTES - 3)))
            .collect::<Vec<_>>();

        let page = dispatcher
            .list(
                worker_epoch(),
                1000,
                &ListStatusRequest {
                    page_size: MAX_STATUS_PAGE_SIZE,
                    cursor: None,
                    filter: None,
                },
                current.clone(),
                names.iter().map(|name| snapshot(name, &current)).collect(),
            )
            .unwrap();

        assert!(!page.workloads.is_empty());
        assert!(page.workloads.len() < names.len());
        assert!(page.next_cursor.is_some());
    }

    #[cfg(feature = "worker-test-fixtures")]
    #[test]
    fn mock_manager_and_runtime_adapters_return_only_typed_manifest_bound_evidence() {
        let selector = BackendSelector {
            workload_name: ObservationText::parse("alpha").unwrap(),
            generated_service: ObservationText::parse("alpha.service").unwrap(),
            container_name: ObservationText::parse("alpha").unwrap(),
        };
        let manager_layers = ManagerLayers {
            generated: Availability::Fresh(GeneratedState {
                service_present: true,
                shadowed: false,
                provenance_matches: true,
            }),
            manager: Availability::Fresh(ManagerState {
                unit_present: true,
                active_state: Some(crate::worker::observation::ManagerActiveState::ActiveRunning),
                terminal_failure: false,
            }),
        };
        let manager = MockManagerAdapter::new(
            BTreeMap::from([(String::from("alpha"), manager_layers.clone())]),
            ManagerLayers {
                generated: Availability::Unavailable,
                manager: Availability::Unavailable,
            },
        );
        let runtime_observation = Availability::Fresh(RuntimeObservation {
            state: crate::worker::observation::RuntimeState::Running,
            identity_matches: true,
        });
        let runtime = MockRuntimeAdapter::new(
            BTreeMap::from([(String::from("alpha"), runtime_observation.clone())]),
            Availability::Unavailable,
        );

        assert_eq!(manager.observe(&selector), manager_layers);
        assert_eq!(runtime.observe(&selector), runtime_observation);
    }

    #[test]
    fn typed_snapshot_serialization_contains_no_raw_backend_payload() {
        let generation = generation('a');
        let encoded = serde_json::to_string(&snapshot("alpha", &generation)).unwrap();

        assert!(!encoded.contains("dbus"));
        assert!(!encoded.contains("podman"));
        assert!(!encoded.contains("cgroup"));
        assert!(!encoded.contains("environment"));
        assert!(!encoded.contains("command"));
    }
}
