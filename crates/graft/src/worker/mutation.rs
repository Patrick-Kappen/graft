//! Principal-scoped lifecycle replay registry and per-workload serialization.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::protocol::ConnectionIdentifier;

use super::lifecycle::{
    CallerDeparture, LifecycleAction, LifecycleRequest, LifecycleState, ManagerEpoch,
    OperationIdentifier, OPERATION_PAST_WINDOW_MS,
};
use super::observation::WorkloadSelector;

/// Maximum retained lifecycle records per principal.
pub const MAX_PRINCIPAL_MUTATIONS: usize = 256;
/// Maximum retained lifecycle records per worker.
pub const MAX_WORKER_MUTATIONS: usize = 1_024;
/// Maximum encoded bytes retained for one terminal lifecycle result.
pub const MAX_RETAINED_RESULT_BYTES: usize = 32 * 1_024;

/// Relationship between one request and manager work.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleDisposition {
    /// Initial state already satisfied the operation.
    NoChange,
    /// This worker submitted manager work.
    WorkerSubmitted,
    /// This worker joined or observed compatible existing work.
    ExistingManagerWork,
}

/// Typed lifecycle failure phase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleFailurePhase {
    /// Manager submission or verified cancellation failed.
    Submission,
    /// Stop or cleanup failed.
    Stop,
    /// Started process or invocation failed.
    Execution,
    /// Correlation evidence became unavailable.
    Observation,
}

/// Authorization class attached to retained lifecycle evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleAuthorization {
    /// Same-effective-UID local user worker authorization.
    OwnUser,
}

/// Immutable bounded terminal lifecycle result retained for duplicates/queries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct LifecycleTerminalResult {
    /// Principal-scoped operation identity.
    pub operation_id: OperationIdentifier,
    /// Worker epoch supplied by the immutable request.
    pub origin_worker_epoch: ConnectionIdentifier,
    /// Current worker epoch retaining this result.
    pub worker_epoch: ConnectionIdentifier,
    /// Manager epoch used for commitment when applicable.
    pub manager_epoch: Option<ManagerEpoch>,
    /// Complete immutable workload selector.
    pub selector: WorkloadSelector,
    /// Declared workload lifecycle.
    pub lifecycle: crate::manifest::WorkloadLifecycle,
    /// Requested action.
    pub action: LifecycleAction,
    /// Authorization class used at acceptance.
    pub authorization: LifecycleAuthorization,
    /// How manager work related to this request.
    pub disposition: LifecycleDisposition,
    /// Proven terminal classification.
    pub outcome: MutationTerminal,
    /// Normalized initial manager state.
    pub initial_state: LifecycleState,
    /// Final manager state when safely observed.
    pub final_state: Option<LifecycleState>,
    /// Correlated manager job identity when known.
    pub job_id: Option<u32>,
    /// Correlated invocation identity when known.
    pub invocation_id: Option<super::observation::ObservationText>,
    /// Server logical acceptance time.
    pub accepted_ms: u64,
    /// Manager submission time, absent for no-change/existing work.
    pub submission_ms: Option<u64>,
    /// Server logical terminal time.
    pub completed_ms: u64,
    /// Whether a dependency affected the terminal result.
    pub dependencies_affected: bool,
    /// Whether manifest generation changed after acceptance.
    pub manifest_changed: bool,
    /// Failure phase when outcome is not success.
    pub failure_phase: Option<LifecycleFailurePhase>,
}

/// Stable pre-commitment terminal error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationTerminalCode {
    /// Caller cancelled before commitment.
    CancelledBeforeCommitment,
    /// Caller deadline elapsed before commitment.
    DeadlineBeforeCommitment,
    /// Caller disconnected before commitment.
    DisconnectedBeforeCommitment,
    /// Previously observed manager job changed before commitment.
    JobChangedBeforeCommitment,
}

/// Accepted operation that ended before manager-work commitment.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct MutationTerminalError {
    /// Principal-scoped operation identity.
    pub operation_id: OperationIdentifier,
    /// Origin worker epoch.
    pub origin_worker_epoch: ConnectionIdentifier,
    /// Current worker epoch retaining the result.
    pub worker_epoch: ConnectionIdentifier,
    /// Immutable selector.
    pub selector: WorkloadSelector,
    /// Requested action.
    pub action: LifecycleAction,
    /// Final caller departure that won commitment, when caller-driven.
    pub departure: Option<CallerDeparture>,
    /// Stable actionable error code.
    pub code: MutationTerminalCode,
    /// Phase at which the operation terminalized.
    pub phase: MutationPhase,
    /// Safe retry guidance.
    pub guidance: MutationGuidance,
    /// Server logical terminal time.
    pub completed_ms: u64,
}

/// Complete retained terminal payload.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "terminal", content = "value", rename_all = "snake_case")]
pub enum RetainedMutationResult {
    /// Manager/no-change lifecycle result.
    Lifecycle(LifecycleTerminalResult),
    /// Pre-commitment terminal error without manager disposition.
    BeforeCommitment(MutationTerminalError),
}

/// Current accepted operation phase exposed by result queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationPhase {
    /// Accepted under the shared activation lock.
    Accepted,
    /// Manager-work commitment is in progress.
    Committing,
    /// Submitted or joined work is being observed.
    Observing,
}

/// Bounded terminal classification retained by the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationTerminal {
    /// Action-specific terminal success was proven.
    Succeeded,
    /// Action-specific terminal failure was proven.
    Failed,
    /// Terminal result could not be proven.
    ResultUnknown,
    /// Final caller departed before manager-work commitment.
    DepartedBeforeCommitment,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum MutationState {
    Reserved,
    InProgress(MutationPhase),
    Terminal {
        result: Box<RetainedMutationResult>,
        completed_ms: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MutationRecord {
    request: LifecycleRequest,
    accepted_ms: u64,
    disposition: Option<LifecycleDisposition>,
    state: MutationState,
}

/// Admission result for a lifecycle mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationAdmission {
    /// New immutable operation was accepted.
    Accepted,
    /// Identical identity is reserved but not durably accepted.
    JoinedReservation,
    /// Identical operation is already in progress.
    JoinedInProgress(MutationPhase),
    /// Identical terminal result remains retained.
    JoinedTerminal(Box<RetainedMutationResult>),
    /// Same principal/ID was used with different immutable fields.
    IdentityConflict,
    /// Another operation owns this workload's mutation slot.
    WorkloadBusy(OperationIdentifier),
    /// Unknown identifier is outside its timestamp window.
    Expired,
    /// Supplied origin epoch cannot submit into this worker epoch.
    OldEpoch,
    /// Principal or worker record capacity is exhausted.
    Overloaded,
}

/// Safe client action for a non-terminal query result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationGuidance {
    /// Poll the same immutable operation identity.
    Poll,
    /// Submit a new operation identity only when still desired.
    SubmitNewOperation,
    /// Do not retry until operator reconciliation restores provenance.
    Reconcile,
    /// Correct immutable query fields before retrying.
    CorrectIdentity,
}

/// Stable typed code for a non-terminal query result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MutationQueryCode {
    /// Operation remains accepted.
    InProgress,
    /// Current-epoch operation was not found.
    NotFound,
    /// Operation identifier is outside its replay window.
    OperationIdExpired,
    /// Originating worker cache is unavailable.
    CacheLost,
    /// Known immutable identity does not match.
    QueryIdentityMismatch,
}

/// Correlated payload for an accepted non-terminal operation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct MutationInProgress {
    /// Principal-scoped operation identity.
    pub operation_id: OperationIdentifier,
    /// Origin worker epoch supplied by the request.
    pub origin_worker_epoch: ConnectionIdentifier,
    /// Current worker epoch answering the query.
    pub worker_epoch: ConnectionIdentifier,
    /// Immutable selector.
    pub selector: WorkloadSelector,
    /// Requested action.
    pub action: LifecycleAction,
    /// Server logical acceptance time.
    pub accepted_ms: u64,
    /// Current typed phase.
    pub phase: MutationPhase,
    /// Manager-work relationship once commitment began.
    pub disposition: Option<LifecycleDisposition>,
    /// Stable result code.
    pub code: MutationQueryCode,
    /// Safe client action.
    pub guidance: MutationGuidance,
}

/// Correlated payload for a recordless or mismatched query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct MutationQueryError {
    /// Queried operation identity.
    pub operation_id: OperationIdentifier,
    /// Origin worker epoch supplied by the request.
    pub origin_worker_epoch: ConnectionIdentifier,
    /// Current worker epoch answering the query.
    pub worker_epoch: ConnectionIdentifier,
    /// Server logical query time.
    pub observed_ms: u64,
    /// Stable result code.
    pub code: MutationQueryCode,
    /// Safe client action.
    pub guidance: MutationGuidance,
}

/// Result of a non-mutating operation-result query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "query_result", content = "value", rename_all = "snake_case")]
pub enum MutationQuery {
    /// Retained terminal result.
    Terminal(Box<RetainedMutationResult>),
    /// Current accepted phase.
    InProgress(MutationInProgress),
    /// Current-epoch identifier is fresh but unknown.
    NotFound(MutationQueryError),
    /// Current-epoch identifier is unknown and expired.
    Expired(MutationQueryError),
    /// Originating worker epoch was lost.
    CacheLost(MutationQueryError),
    /// Known ID does not match action or selector.
    IdentityMismatch(MutationQueryError),
}

/// Bounded in-memory operation registry; not desired-state persistence.
#[derive(Debug, Clone)]
pub struct MutationRegistry {
    worker_epoch: ConnectionIdentifier,
    records: BTreeMap<(u32, OperationIdentifier), MutationRecord>,
    active_workloads: BTreeMap<(u32, WorkloadSelector), OperationIdentifier>,
}

impl MutationRegistry {
    /// Creates an empty registry owned by one worker epoch.
    #[must_use]
    pub fn new(worker_epoch: ConnectionIdentifier) -> Self {
        Self {
            worker_epoch,
            records: BTreeMap::new(),
            active_workloads: BTreeMap::new(),
        }
    }

    /// Admits, joins, or rejects one immutable mutation.
    #[must_use]
    pub fn admit(
        &mut self,
        principal_uid: u32,
        request: LifecycleRequest,
        logical_now_ms: u64,
    ) -> MutationAdmission {
        self.expire(logical_now_ms);
        let key = (principal_uid, request.operation_id);
        if let Some(record) = self.records.get(&key) {
            if record.request != request {
                return MutationAdmission::IdentityConflict;
            }
            return match record.state {
                MutationState::Reserved => MutationAdmission::JoinedReservation,
                MutationState::InProgress(phase) => MutationAdmission::JoinedInProgress(phase),
                MutationState::Terminal { ref result, .. } => {
                    MutationAdmission::JoinedTerminal(result.clone())
                }
            };
        }
        if request.origin_worker_epoch != self.worker_epoch {
            return MutationAdmission::OldEpoch;
        }
        if !request.operation_id.is_fresh_at(logical_now_ms) {
            return MutationAdmission::Expired;
        }
        let workload_key = (principal_uid, request.selector.clone());
        if let Some(operation_id) = self.active_workloads.get(&workload_key) {
            return MutationAdmission::WorkloadBusy(*operation_id);
        }
        let principal_count = self
            .records
            .keys()
            .filter(|(uid, _)| *uid == principal_uid)
            .count();
        if principal_count >= MAX_PRINCIPAL_MUTATIONS || self.records.len() >= MAX_WORKER_MUTATIONS
        {
            return MutationAdmission::Overloaded;
        }
        self.active_workloads
            .insert(workload_key, request.operation_id);
        self.records.insert(
            key,
            MutationRecord {
                request,
                accepted_ms: logical_now_ms,
                disposition: None,
                state: MutationState::Reserved,
            },
        );
        MutationAdmission::Accepted
    }

    /// Publishes durable acceptance for one reserved identity.
    #[must_use]
    pub fn accept(&mut self, principal_uid: u32, operation_id: OperationIdentifier) -> bool {
        let Some(record) = self.records.get_mut(&(principal_uid, operation_id)) else {
            return false;
        };
        if record.state != MutationState::Reserved {
            return false;
        }
        record.state = MutationState::InProgress(MutationPhase::Accepted);
        true
    }

    /// Removes a reservation or accepted operation before manager commitment.
    #[must_use]
    pub fn reject_accepted(
        &mut self,
        principal_uid: u32,
        operation_id: OperationIdentifier,
    ) -> bool {
        let key = (principal_uid, operation_id);
        let Some(record) = self.records.get(&key) else {
            return false;
        };
        if !matches!(
            record.state,
            MutationState::Reserved | MutationState::InProgress(MutationPhase::Accepted)
        ) {
            return false;
        }
        let workload_key = (principal_uid, record.request.selector.clone());
        self.active_workloads.remove(&workload_key);
        self.records.remove(&key).is_some()
    }

    /// Records manager-work disposition and advances commitment.
    #[must_use]
    pub fn commit(
        &mut self,
        principal_uid: u32,
        operation_id: OperationIdentifier,
        disposition: LifecycleDisposition,
    ) -> bool {
        let Some(record) = self.records.get_mut(&(principal_uid, operation_id)) else {
            return false;
        };
        if matches!(record.state, MutationState::Terminal { .. }) {
            return false;
        }
        record.disposition = Some(disposition);
        record.state = MutationState::InProgress(MutationPhase::Committing);
        true
    }

    /// Advances one known in-progress operation.
    #[must_use]
    pub fn advance(
        &mut self,
        principal_uid: u32,
        operation_id: OperationIdentifier,
        phase: MutationPhase,
    ) -> bool {
        let Some(record) = self.records.get_mut(&(principal_uid, operation_id)) else {
            return false;
        };
        if matches!(record.state, MutationState::Terminal { .. }) {
            return false;
        }
        record.state = MutationState::InProgress(phase);
        true
    }

    /// Publishes an immutable terminal result and releases the workload slot.
    #[must_use]
    pub fn terminal(
        &mut self,
        principal_uid: u32,
        operation_id: OperationIdentifier,
        result: RetainedMutationResult,
        logical_now_ms: u64,
    ) -> bool {
        if serde_json::to_vec(&result)
            .map_or(true, |encoded| encoded.len() > MAX_RETAINED_RESULT_BYTES)
        {
            return false;
        }
        let Some(record) = self.records.get_mut(&(principal_uid, operation_id)) else {
            return false;
        };
        if matches!(record.state, MutationState::Terminal { .. }) {
            return false;
        }
        let workload_key = (principal_uid, record.request.selector.clone());
        record.state = MutationState::Terminal {
            result: Box::new(result),
            completed_ms: logical_now_ms,
        };
        self.active_workloads.remove(&workload_key);
        true
    }

    /// Queries without submitting, joining, or changing lifecycle work.
    #[must_use]
    pub fn query(
        &mut self,
        principal_uid: u32,
        request: &LifecycleRequest,
        logical_now_ms: u64,
    ) -> MutationQuery {
        self.expire(logical_now_ms);
        let query_error = |code, guidance| MutationQueryError {
            operation_id: request.operation_id,
            origin_worker_epoch: request.origin_worker_epoch,
            worker_epoch: self.worker_epoch,
            observed_ms: logical_now_ms,
            code,
            guidance,
        };
        if request.origin_worker_epoch != self.worker_epoch {
            return MutationQuery::CacheLost(query_error(
                MutationQueryCode::CacheLost,
                MutationGuidance::Reconcile,
            ));
        }
        let Some(record) = self.records.get(&(principal_uid, request.operation_id)) else {
            return if request.operation_id.is_fresh_at(logical_now_ms) {
                MutationQuery::NotFound(query_error(
                    MutationQueryCode::NotFound,
                    MutationGuidance::SubmitNewOperation,
                ))
            } else {
                MutationQuery::Expired(query_error(
                    MutationQueryCode::OperationIdExpired,
                    MutationGuidance::SubmitNewOperation,
                ))
            };
        };
        if record.request.action != request.action || record.request.selector != request.selector {
            return MutationQuery::IdentityMismatch(query_error(
                MutationQueryCode::QueryIdentityMismatch,
                MutationGuidance::CorrectIdentity,
            ));
        }
        match record.state {
            MutationState::Reserved => MutationQuery::NotFound(query_error(
                MutationQueryCode::NotFound,
                MutationGuidance::Poll,
            )),
            MutationState::InProgress(phase) => MutationQuery::InProgress(MutationInProgress {
                operation_id: request.operation_id,
                origin_worker_epoch: request.origin_worker_epoch,
                worker_epoch: self.worker_epoch,
                selector: request.selector.clone(),
                action: request.action,
                accepted_ms: record.accepted_ms,
                phase,
                disposition: record.disposition,
                code: MutationQueryCode::InProgress,
                guidance: MutationGuidance::Poll,
            }),
            MutationState::Terminal { ref result, .. } => MutationQuery::Terminal(result.clone()),
        }
    }

    fn expire(&mut self, logical_now_ms: u64) {
        self.records.retain(|(uid, _), record| {
            let keep = match record.state {
                MutationState::Reserved | MutationState::InProgress(_) => true,
                MutationState::Terminal { completed_ms, .. } => {
                    let acceptance_boundary =
                        record.accepted_ms.saturating_add(OPERATION_PAST_WINDOW_MS);
                    let identifier_boundary = record
                        .request
                        .operation_id
                        .timestamp_ms()
                        .saturating_add(OPERATION_PAST_WINDOW_MS);
                    let completion_boundary = completed_ms.saturating_add(OPERATION_PAST_WINDOW_MS);
                    logical_now_ms
                        <= acceptance_boundary
                            .max(identifier_boundary)
                            .max(completion_boundary)
                }
            };
            if !keep {
                self.active_workloads
                    .remove(&(*uid, record.request.selector.clone()));
            }
            keep
        });
    }
}

#[cfg(test)]
mod tests {
    use crate::protocol::{ManifestGeneration, WorkerTarget};
    use crate::worker::lifecycle::LifecycleAction;
    use crate::worker::observation::ObservationText;

    use super::*;

    fn epoch(value: &str) -> ConnectionIdentifier {
        ConnectionIdentifier::parse(value).unwrap()
    }

    fn request(epoch: ConnectionIdentifier, operation_id: &str, name: &str) -> LifecycleRequest {
        LifecycleRequest {
            operation_id: OperationIdentifier::parse(operation_id).unwrap(),
            origin_worker_epoch: epoch,
            selector: WorkloadSelector {
                target: WorkerTarget::User,
                name: ObservationText::parse(name).unwrap(),
                generation: ManifestGeneration::parse("a".repeat(64)).unwrap(),
                workload_id: ManifestGeneration::parse("b".repeat(64)).unwrap(),
            },
            action: LifecycleAction::Up,
        }
    }

    fn terminal_result(
        request: &LifecycleRequest,
        outcome: MutationTerminal,
        now: u64,
    ) -> RetainedMutationResult {
        RetainedMutationResult::Lifecycle(LifecycleTerminalResult {
            operation_id: request.operation_id,
            origin_worker_epoch: request.origin_worker_epoch,
            worker_epoch: request.origin_worker_epoch,
            manager_epoch: None,
            selector: request.selector.clone(),
            lifecycle: crate::manifest::WorkloadLifecycle::LongRunning,
            action: request.action,
            authorization: LifecycleAuthorization::OwnUser,
            disposition: LifecycleDisposition::WorkerSubmitted,
            outcome,
            initial_state: LifecycleState::Inactive,
            final_state: Some(LifecycleState::ActiveRunning),
            job_id: Some(7),
            invocation_id: None,
            accepted_ms: now,
            submission_ms: Some(now),
            completed_ms: now,
            dependencies_affected: false,
            manifest_changed: false,
            failure_phase: None,
        })
    }

    #[test]
    fn identical_duplicates_join_and_changed_identity_conflicts() {
        let worker_epoch = epoch("018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20");
        let value = request(
            worker_epoch,
            "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c21",
            "alpha",
        );
        let now = value.operation_id.timestamp_ms();
        let mut registry = MutationRegistry::new(worker_epoch);

        assert_eq!(
            registry.admit(1000, value.clone(), now),
            MutationAdmission::Accepted
        );
        assert_eq!(
            registry.admit(1000, value.clone(), now),
            MutationAdmission::JoinedReservation
        );
        assert!(registry.accept(1000, value.operation_id));
        assert_eq!(
            registry.admit(1000, value.clone(), now),
            MutationAdmission::JoinedInProgress(MutationPhase::Accepted)
        );
        let mut changed = value.clone();
        changed.action = LifecycleAction::Down;
        assert_eq!(
            registry.admit(1000, changed, now),
            MutationAdmission::IdentityConflict
        );
        assert_eq!(
            registry.admit(1001, value, now),
            MutationAdmission::Accepted
        );
    }

    #[test]
    fn workload_lock_terminal_retention_and_query_are_independent() {
        let worker_epoch = epoch("018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20");
        let first = request(
            worker_epoch,
            "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c21",
            "alpha",
        );
        let second = request(
            worker_epoch,
            "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c22",
            "alpha",
        );
        let now = first.operation_id.timestamp_ms();
        let mut registry = MutationRegistry::new(worker_epoch);
        assert_eq!(
            registry.admit(1000, first.clone(), now),
            MutationAdmission::Accepted
        );
        assert!(matches!(
            registry.admit(1000, second.clone(), now),
            MutationAdmission::WorkloadBusy(_)
        ));
        assert!(registry.advance(1000, first.operation_id, MutationPhase::Observing));
        let terminal = terminal_result(&first, MutationTerminal::Succeeded, now);
        assert!(registry.terminal(1000, first.operation_id, terminal.clone(), now));
        assert_eq!(
            registry.query(1000, &first, now),
            MutationQuery::Terminal(Box::new(terminal))
        );
        assert_eq!(
            registry.admit(1000, second, now),
            MutationAdmission::Accepted
        );
    }

    #[test]
    fn old_epoch_and_unknown_expired_queries_never_submit() {
        let worker_epoch = epoch("018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20");
        let old_epoch = epoch("018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c29");
        let value = request(old_epoch, "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c21", "alpha");
        let now = value.operation_id.timestamp_ms();
        let mut registry = MutationRegistry::new(worker_epoch);

        assert_eq!(
            registry.admit(1000, value.clone(), now),
            MutationAdmission::OldEpoch
        );
        assert!(matches!(
            registry.query(1000, &value, now),
            MutationQuery::CacheLost(MutationQueryError {
                code: MutationQueryCode::CacheLost,
                guidance: MutationGuidance::Reconcile,
                ..
            })
        ));
        let current = LifecycleRequest {
            origin_worker_epoch: worker_epoch,
            ..value
        };
        assert!(matches!(
            registry.query(1000, &current, now),
            MutationQuery::NotFound(MutationQueryError {
                code: MutationQueryCode::NotFound,
                guidance: MutationGuidance::SubmitNewOperation,
                ..
            })
        ));
        assert!(matches!(
            registry.query(1000, &current, now + OPERATION_PAST_WINDOW_MS),
            MutationQuery::Expired(MutationQueryError {
                code: MutationQueryCode::OperationIdExpired,
                guidance: MutationGuidance::SubmitNewOperation,
                ..
            })
        ));
    }
}
