//! Principal-scoped lifecycle replay registry and per-workload serialization.

use std::collections::BTreeMap;

use crate::protocol::ConnectionIdentifier;

use super::lifecycle::{LifecycleRequest, OperationIdentifier, OPERATION_PAST_WINDOW_MS};
use super::observation::WorkloadSelector;

/// Maximum retained lifecycle records per principal.
pub const MAX_PRINCIPAL_MUTATIONS: usize = 256;
/// Maximum retained lifecycle records per worker.
pub const MAX_WORKER_MUTATIONS: usize = 1_024;

/// Current accepted operation phase exposed by result queries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MutationPhase {
    /// Accepted under the shared activation lock.
    Accepted,
    /// Manager-work commitment is in progress.
    Committing,
    /// Submitted or joined work is being observed.
    Observing,
}

/// Bounded terminal classification retained by the registry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    InProgress(MutationPhase),
    Terminal {
        result: MutationTerminal,
        completed_ms: u64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MutationRecord {
    request: LifecycleRequest,
    accepted_ms: u64,
    state: MutationState,
}

/// Admission result for a lifecycle mutation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationAdmission {
    /// New immutable operation was accepted.
    Accepted,
    /// Identical operation is already in progress.
    JoinedInProgress(MutationPhase),
    /// Identical terminal result remains retained.
    JoinedTerminal(MutationTerminal),
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

/// Result of a non-mutating operation-result query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MutationQuery {
    /// Retained terminal result.
    Terminal(MutationTerminal),
    /// Current accepted phase.
    InProgress(MutationPhase),
    /// Current-epoch identifier is fresh but unknown.
    NotFound,
    /// Current-epoch identifier is unknown and expired.
    Expired,
    /// Originating worker epoch was lost.
    CacheLost,
    /// Known ID does not match action or selector.
    IdentityMismatch,
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

    /// Classifies admission without reserving identity or workload state.
    #[must_use]
    pub fn preview(
        &self,
        principal_uid: u32,
        request: &LifecycleRequest,
        logical_now_ms: u64,
    ) -> MutationAdmission {
        let mut candidate = self.clone();
        candidate.admit(principal_uid, request.clone(), logical_now_ms)
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
                MutationState::InProgress(phase) => MutationAdmission::JoinedInProgress(phase),
                MutationState::Terminal { result, .. } => MutationAdmission::JoinedTerminal(result),
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
                state: MutationState::InProgress(MutationPhase::Accepted),
            },
        );
        MutationAdmission::Accepted
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
        result: MutationTerminal,
        logical_now_ms: u64,
    ) -> bool {
        let Some(record) = self.records.get_mut(&(principal_uid, operation_id)) else {
            return false;
        };
        if matches!(record.state, MutationState::Terminal { .. }) {
            return false;
        }
        let workload_key = (principal_uid, record.request.selector.clone());
        record.state = MutationState::Terminal {
            result,
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
        if request.origin_worker_epoch != self.worker_epoch {
            return MutationQuery::CacheLost;
        }
        let Some(record) = self.records.get(&(principal_uid, request.operation_id)) else {
            return if request.operation_id.is_fresh_at(logical_now_ms) {
                MutationQuery::NotFound
            } else {
                MutationQuery::Expired
            };
        };
        if record.request.action != request.action || record.request.selector != request.selector {
            return MutationQuery::IdentityMismatch;
        }
        match record.state {
            MutationState::InProgress(phase) => MutationQuery::InProgress(phase),
            MutationState::Terminal { result, .. } => MutationQuery::Terminal(result),
        }
    }

    fn expire(&mut self, logical_now_ms: u64) {
        self.records.retain(|(uid, _), record| {
            let keep = match record.state {
                MutationState::InProgress(_) => true,
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
        assert!(registry.terminal(1000, first.operation_id, MutationTerminal::Succeeded, now));
        assert_eq!(
            registry.query(1000, &first, now),
            MutationQuery::Terminal(MutationTerminal::Succeeded)
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
        assert_eq!(registry.query(1000, &value, now), MutationQuery::CacheLost);
        let current = LifecycleRequest {
            origin_worker_epoch: worker_epoch,
            ..value
        };
        assert_eq!(registry.query(1000, &current, now), MutationQuery::NotFound);
        assert_eq!(
            registry.query(1000, &current, now + OPERATION_PAST_WINDOW_MS),
            MutationQuery::Expired
        );
    }
}
