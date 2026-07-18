//! Lifecycle commitment coordinator for validated user workloads.

use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};

use crate::manifest::WorkloadLifecycle;
use crate::protocol::ConnectionIdentifier;

use super::discovery::BackendSelector;
use super::interlock::{InterlockPhase, InterlockRecord, InterlockStore};
use super::lifecycle::{
    decide_lifecycle, CallerDeparture, CommitmentGate, CommitmentOutcome, CorrelatedResult,
    LifecycleAction, LifecycleAdapterError, LifecycleDecision, LifecycleManagerAdapter,
    LifecycleRequest, LifecycleState, ManagerAction,
};
use super::mutation::{
    LifecycleAuthorization, LifecycleDisposition, LifecycleFailurePhase, LifecycleTerminalResult,
    MutationAdmission, MutationPhase, MutationQuery, MutationRegistry, MutationTerminal,
    MutationTerminalError, RetainedMutationResult,
};

const MAX_TERMINAL_OBSERVATIONS: usize = 16;

/// Result of one lifecycle submission or duplicate join attempt.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "execution", content = "value", rename_all = "snake_case")]
pub enum LifecycleExecution {
    /// Action-specific terminal result was established.
    Terminal(Box<RetainedMutationResult>),
    /// Identical accepted operation remains in progress.
    InProgress(MutationPhase),
    /// Pre-acceptance request rejection.
    Rejected(LifecycleRejection),
}

/// Closed pre-acceptance lifecycle rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleRejection {
    /// Workload is declaratively disabled.
    WorkloadDisabled,
    /// Operation identity conflicts with retained immutable fields.
    IdentityConflict,
    /// Another operation owns the workload slot.
    OperationInProgress,
    /// Operation identifier is outside its replay window.
    OperationExpired,
    /// Origin worker epoch is no longer current.
    OldWorkerEpoch,
    /// Registry or interlock capacity is exhausted.
    Overloaded,
    /// Manager state conflicts with this action.
    Conflict,
    /// Manager state is incompatible with lifecycle intent.
    UnexpectedState,
    /// Manager, lock, or interlock infrastructure is unavailable.
    InfrastructureUnavailable,
}

/// Bounded startup/on-demand interlock reconciliation result.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ReconciliationReport {
    /// Safely cleared prepared or terminal records.
    pub cleared: usize,
    /// Records retained because manager evidence remains active or ambiguous.
    pub retained: usize,
}

/// Worker-side lifecycle coordinator with no direct runtime fallback.
#[derive(Debug)]
pub struct LifecycleCoordinator<M: LifecycleManagerAdapter> {
    manager: M,
    interlocks: InterlockStore,
    registry: Mutex<MutationRegistry>,
}

impl<M: LifecycleManagerAdapter> LifecycleCoordinator<M> {
    /// Creates one coordinator for a fixed worker epoch and manager context.
    #[must_use]
    pub fn new(worker_epoch: ConnectionIdentifier, manager: M, interlocks: InterlockStore) -> Self {
        Self {
            manager,
            interlocks,
            registry: Mutex::new(MutationRegistry::new(worker_epoch)),
        }
    }

    /// Queries retained operation state without manager submission or joining.
    #[must_use]
    pub fn query(
        &self,
        principal_uid: u32,
        request: &LifecycleRequest,
        logical_now_ms: u64,
    ) -> MutationQuery {
        let Ok(mut registry) = self.registry.lock() else {
            return MutationQuery::CacheLost;
        };
        registry.query(principal_uid, request, logical_now_ms)
    }

    /// Reconciles durable records without reconstructing mutation results.
    ///
    /// # Errors
    ///
    /// Returns infrastructure unavailable when records, lock, or manager identity
    /// cannot be loaded safely. Ambiguous individual records remain retained.
    pub fn reconcile_interlocks(&self) -> Result<ReconciliationReport, LifecycleRejection> {
        let _activation = self
            .interlocks
            .lock_submission()
            .map_err(|_| LifecycleRejection::InfrastructureUnavailable)?;
        let records = self
            .interlocks
            .load()
            .map_err(|_| LifecycleRejection::InfrastructureUnavailable)?;
        let manager_epoch = self
            .manager
            .epoch()
            .map_err(|_| LifecycleRejection::InfrastructureUnavailable)?;
        let mut report = ReconciliationReport {
            cleared: 0,
            retained: 0,
        };
        for record in records {
            let clear = if record.phase == InterlockPhase::Prepared {
                true
            } else if record.manager_epoch.as_ref() != Some(&manager_epoch) {
                false
            } else {
                self.manager
                    .observe(&record.backend_selector)
                    .ok()
                    .is_some_and(|observation| reconciliation_terminal(&record, observation))
            };
            if clear {
                self.interlocks
                    .remove(record.operation_id)
                    .map_err(|_| LifecycleRejection::InfrastructureUnavailable)?;
                report.cleared = report.cleared.saturating_add(1);
            } else {
                report.retained = report.retained.saturating_add(1);
            }
        }
        Ok(report)
    }

    /// Executes one already-authorized request with an initially interested caller.
    #[must_use]
    pub fn execute(
        &self,
        principal_uid: u32,
        request: LifecycleRequest,
        backend_selector: &BackendSelector,
        lifecycle: WorkloadLifecycle,
        enabled: bool,
        logical_now_ms: u64,
    ) -> LifecycleExecution {
        self.execute_with_gate(
            principal_uid,
            request,
            backend_selector,
            lifecycle,
            enabled,
            logical_now_ms,
            &Mutex::new(CommitmentGate::new()),
        )
    }

    /// Executes with caller interest serialized against manager commitment.
    ///
    /// Manager acceptance is never terminal success. If final caller departure
    /// wins the gate, no manager attachment or backend call occurs.
    #[must_use]
    #[allow(clippy::too_many_arguments)]
    pub fn execute_with_gate(
        &self,
        principal_uid: u32,
        request: LifecycleRequest,
        backend_selector: &BackendSelector,
        lifecycle: WorkloadLifecycle,
        enabled: bool,
        logical_now_ms: u64,
        commitment: &Mutex<CommitmentGate>,
    ) -> LifecycleExecution {
        if !enabled {
            return LifecycleExecution::Rejected(LifecycleRejection::WorkloadDisabled);
        }
        let Ok(mut registry) = self.registry.lock() else {
            return LifecycleExecution::Rejected(LifecycleRejection::InfrastructureUnavailable);
        };
        match registry.preview(principal_uid, &request, logical_now_ms) {
            MutationAdmission::JoinedInProgress(phase) => {
                return LifecycleExecution::InProgress(phase);
            }
            MutationAdmission::JoinedTerminal(result) => {
                return LifecycleExecution::Terminal(result);
            }
            MutationAdmission::IdentityConflict => {
                return LifecycleExecution::Rejected(LifecycleRejection::IdentityConflict);
            }
            MutationAdmission::WorkloadBusy(_) => {
                return LifecycleExecution::Rejected(LifecycleRejection::OperationInProgress);
            }
            MutationAdmission::Expired => {
                return LifecycleExecution::Rejected(LifecycleRejection::OperationExpired);
            }
            MutationAdmission::OldEpoch => {
                return LifecycleExecution::Rejected(LifecycleRejection::OldWorkerEpoch);
            }
            MutationAdmission::Overloaded => {
                return LifecycleExecution::Rejected(LifecycleRejection::Overloaded);
            }
            MutationAdmission::Accepted => {}
        }
        self.execute_new(
            &mut registry,
            principal_uid,
            request,
            backend_selector,
            lifecycle,
            logical_now_ms,
            commitment,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_new(
        &self,
        registry: &mut MutexGuard<'_, MutationRegistry>,
        principal_uid: u32,
        request: LifecycleRequest,
        backend_selector: &BackendSelector,
        lifecycle: WorkloadLifecycle,
        logical_now_ms: u64,
        commitment: &Mutex<CommitmentGate>,
    ) -> LifecycleExecution {
        let Ok(_activation) = self.interlocks.lock_submission() else {
            return LifecycleExecution::Rejected(LifecycleRejection::InfrastructureUnavailable);
        };
        let Ok(retained) = self.interlocks.load() else {
            return LifecycleExecution::Rejected(LifecycleRejection::InfrastructureUnavailable);
        };
        if retained.iter().any(|record| {
            record.selector == request.selector && record.operation_id != request.operation_id
        }) {
            return LifecycleExecution::Rejected(LifecycleRejection::OperationInProgress);
        }
        let Ok(manager_epoch) = self.manager.epoch() else {
            return LifecycleExecution::Rejected(LifecycleRejection::InfrastructureUnavailable);
        };
        let Ok(observation) = self.manager.observe(backend_selector) else {
            return LifecycleExecution::Rejected(LifecycleRejection::InfrastructureUnavailable);
        };
        let decision = decide_lifecycle(request.action, lifecycle, observation);
        match decision {
            LifecycleDecision::Conflict => {
                LifecycleExecution::Rejected(LifecycleRejection::Conflict)
            }
            LifecycleDecision::UnexpectedState => {
                LifecycleExecution::Rejected(LifecycleRejection::UnexpectedState)
            }
            LifecycleDecision::BackendUnavailable => {
                LifecycleExecution::Rejected(LifecycleRejection::InfrastructureUnavailable)
            }
            LifecycleDecision::NoChange => {
                if registry.admit(principal_uid, request.clone(), logical_now_ms)
                    != MutationAdmission::Accepted
                {
                    return LifecycleExecution::Rejected(LifecycleRejection::OperationInProgress);
                }
                let terminal = Self::lifecycle_result(
                    &request,
                    lifecycle,
                    Some(manager_epoch),
                    LifecycleDisposition::NoChange,
                    MutationTerminal::Succeeded,
                    observation.state,
                    Some(observation.state),
                    None,
                    logical_now_ms,
                    None,
                );
                let _ = registry.terminal(
                    principal_uid,
                    request.operation_id,
                    terminal.clone(),
                    logical_now_ms,
                );
                LifecycleExecution::Terminal(Box::new(terminal))
            }
            LifecycleDecision::Submit(action) => self.commit(
                registry,
                principal_uid,
                request,
                backend_selector,
                lifecycle,
                logical_now_ms,
                manager_epoch,
                Some(action),
                observation.queued_job.map(|job| job.job_id),
                observation.state,
                commitment,
            ),
            LifecycleDecision::JoinExisting | LifecycleDecision::ObserveExisting => self.commit(
                registry,
                principal_uid,
                request,
                backend_selector,
                lifecycle,
                logical_now_ms,
                manager_epoch,
                None,
                observation.queued_job.map(|job| job.job_id),
                observation.state,
                commitment,
            ),
        }
    }

    #[allow(
        clippy::too_many_arguments,
        clippy::too_many_lines,
        clippy::needless_pass_by_value
    )]
    fn commit(
        &self,
        registry: &mut MutationRegistry,
        principal_uid: u32,
        request: LifecycleRequest,
        backend_selector: &BackendSelector,
        lifecycle: WorkloadLifecycle,
        logical_now_ms: u64,
        manager_epoch: super::lifecycle::ManagerEpoch,
        mut manager_action: Option<ManagerAction>,
        verified_job_id: Option<u32>,
        initial_state: LifecycleState,
        commitment: &Mutex<CommitmentGate>,
    ) -> LifecycleExecution {
        let disposition = if manager_action.is_some() {
            LifecycleDisposition::WorkerSubmitted
        } else {
            LifecycleDisposition::ExistingManagerWork
        };
        let mut interlock = InterlockRecord {
            worker_epoch: request.origin_worker_epoch,
            principal_uid,
            operation_id: request.operation_id,
            selector: request.selector.clone(),
            lifecycle,
            action: request.action,
            backend_selector: backend_selector.clone(),
            initial_state,
            phase: InterlockPhase::Prepared,
            manager_epoch: Some(manager_epoch.clone()),
            job_id: verified_job_id,
            invocation_id: None,
        };
        if self.interlocks.persist(&interlock).is_err() {
            return LifecycleExecution::Rejected(LifecycleRejection::InfrastructureUnavailable);
        }
        if registry.admit(principal_uid, request.clone(), logical_now_ms)
            != MutationAdmission::Accepted
        {
            let _ = self.interlocks.remove(request.operation_id);
            return LifecycleExecution::Rejected(LifecycleRejection::OperationInProgress);
        }
        interlock.phase = match manager_action {
            Some(ManagerAction::CancelStartThenStop) => InterlockPhase::CommittingCancel,
            Some(_) => InterlockPhase::CommittingSubmission,
            None => InterlockPhase::ObservingExisting,
        };
        if self.interlocks.persist(&interlock).is_err() {
            return Self::unknown(
                registry,
                principal_uid,
                &request,
                &interlock,
                initial_state,
                disposition,
                logical_now_ms,
            );
        }
        let _ = registry.advance(
            principal_uid,
            request.operation_id,
            MutationPhase::Committing,
        );
        let commitment_outcome = match commitment.lock() {
            Ok(mut gate) => gate.commit(),
            Err(_) => CommitmentOutcome::Departed(CallerDeparture::Disconnected),
        };
        if let CommitmentOutcome::Departed(departure) = commitment_outcome {
            let _ = self.interlocks.remove(request.operation_id);
            let terminal = RetainedMutationResult::BeforeCommitment(MutationTerminalError {
                operation_id: request.operation_id,
                origin_worker_epoch: request.origin_worker_epoch,
                selector: request.selector.clone(),
                action: request.action,
                departure,
                completed_ms: logical_now_ms,
            });
            let _ = registry.terminal(
                principal_uid,
                request.operation_id,
                terminal.clone(),
                logical_now_ms,
            );
            return LifecycleExecution::Terminal(Box::new(terminal));
        }
        if manager_action == Some(ManagerAction::CancelStartThenStop) {
            let Some(job_id) = verified_job_id else {
                return Self::unknown(
                    registry,
                    principal_uid,
                    &request,
                    &interlock,
                    initial_state,
                    disposition,
                    logical_now_ms,
                );
            };
            match self.manager.cancel_start(backend_selector, job_id) {
                Ok(()) => {
                    interlock.phase = InterlockPhase::CancelCommittedStopPending;
                    if self.interlocks.persist(&interlock).is_err() {
                        return Self::unknown(
                            registry,
                            principal_uid,
                            &request,
                            &interlock,
                            initial_state,
                            disposition,
                            logical_now_ms,
                        );
                    }
                    interlock.phase = InterlockPhase::CommittingStop;
                    if self.interlocks.persist(&interlock).is_err() {
                        return Self::unknown(
                            registry,
                            principal_uid,
                            &request,
                            &interlock,
                            initial_state,
                            disposition,
                            logical_now_ms,
                        );
                    }
                    manager_action = Some(ManagerAction::Stop);
                }
                Err(LifecycleAdapterError::Rejected) => {
                    let terminal = Self::lifecycle_result(
                        &request,
                        lifecycle,
                        Some(manager_epoch.clone()),
                        LifecycleDisposition::WorkerSubmitted,
                        MutationTerminal::Failed,
                        initial_state,
                        None,
                        interlock.job_id,
                        logical_now_ms,
                        Some(LifecycleFailurePhase::Submission),
                    );
                    let _ = registry.terminal(
                        principal_uid,
                        request.operation_id,
                        terminal.clone(),
                        logical_now_ms,
                    );
                    return LifecycleExecution::Terminal(Box::new(terminal));
                }
                _ => {
                    return Self::unknown(
                        registry,
                        principal_uid,
                        &request,
                        &interlock,
                        initial_state,
                        disposition,
                        logical_now_ms,
                    );
                }
            }
        }
        if let Some(action) = manager_action {
            match self.manager.submit(backend_selector, action) {
                Ok(submission) if submission.epoch == manager_epoch => {
                    interlock.job_id = submission.job_id;
                    interlock.phase = InterlockPhase::CommittedSubmission;
                    if self.interlocks.persist(&interlock).is_err() {
                        return Self::unknown(
                            registry,
                            principal_uid,
                            &request,
                            &interlock,
                            initial_state,
                            disposition,
                            logical_now_ms,
                        );
                    }
                }
                Err(LifecycleAdapterError::Rejected) => {
                    let _ = self.interlocks.remove(request.operation_id);
                    let terminal = Self::lifecycle_result(
                        &request,
                        lifecycle,
                        Some(manager_epoch.clone()),
                        LifecycleDisposition::WorkerSubmitted,
                        MutationTerminal::Failed,
                        initial_state,
                        None,
                        interlock.job_id,
                        logical_now_ms,
                        Some(LifecycleFailurePhase::Submission),
                    );
                    let _ = registry.terminal(
                        principal_uid,
                        request.operation_id,
                        terminal.clone(),
                        logical_now_ms,
                    );
                    return LifecycleExecution::Terminal(Box::new(terminal));
                }
                _ => {
                    return Self::unknown(
                        registry,
                        principal_uid,
                        &request,
                        &interlock,
                        initial_state,
                        disposition,
                        logical_now_ms,
                    );
                }
            }
        }
        let _ = registry.advance(
            principal_uid,
            request.operation_id,
            MutationPhase::Observing,
        );
        let mut outcome = MutationTerminal::ResultUnknown;
        let mut final_state = None;
        let mut failure_phase = Some(LifecycleFailurePhase::Observation);
        for _ in 0..MAX_TERMINAL_OBSERVATIONS {
            let (Ok(observation), Ok(observed_epoch)) =
                (self.manager.observe(backend_selector), self.manager.epoch())
            else {
                break;
            };
            if observed_epoch != manager_epoch {
                break;
            }
            final_state = Some(observation.state);
            if terminal_satisfied(request.action, lifecycle, observation) {
                outcome = MutationTerminal::Succeeded;
                failure_phase = None;
                break;
            }
            if observation.execution_result == CorrelatedResult::Failed
                || observation.stop_result == CorrelatedResult::Failed
            {
                outcome = MutationTerminal::Failed;
                failure_phase = Some(if observation.stop_result == CorrelatedResult::Failed {
                    LifecycleFailurePhase::Stop
                } else {
                    LifecycleFailurePhase::Execution
                });
                break;
            }
        }
        if outcome == MutationTerminal::ResultUnknown {
            return LifecycleExecution::InProgress(MutationPhase::Observing);
        }
        let _ = self.interlocks.remove(request.operation_id);
        let terminal = Self::lifecycle_result(
            &request,
            lifecycle,
            Some(manager_epoch),
            disposition,
            outcome,
            initial_state,
            final_state,
            interlock.job_id,
            logical_now_ms,
            failure_phase,
        );
        let _ = registry.terminal(
            principal_uid,
            request.operation_id,
            terminal.clone(),
            logical_now_ms,
        );
        LifecycleExecution::Terminal(Box::new(terminal))
    }

    #[allow(clippy::too_many_arguments)]
    fn unknown(
        registry: &mut MutationRegistry,
        principal_uid: u32,
        request: &LifecycleRequest,
        interlock: &InterlockRecord,
        initial_state: LifecycleState,
        disposition: LifecycleDisposition,
        logical_now_ms: u64,
    ) -> LifecycleExecution {
        let terminal = Self::lifecycle_result(
            request,
            interlock.lifecycle,
            interlock.manager_epoch.clone(),
            disposition,
            MutationTerminal::ResultUnknown,
            initial_state,
            None,
            interlock.job_id,
            logical_now_ms,
            Some(LifecycleFailurePhase::Observation),
        );
        let _ = registry.terminal(
            principal_uid,
            request.operation_id,
            terminal.clone(),
            logical_now_ms,
        );
        LifecycleExecution::Terminal(Box::new(terminal))
    }

    #[allow(clippy::too_many_arguments)]
    fn lifecycle_result(
        request: &LifecycleRequest,
        lifecycle: WorkloadLifecycle,
        manager_epoch: Option<super::lifecycle::ManagerEpoch>,
        disposition: LifecycleDisposition,
        outcome: MutationTerminal,
        initial_state: LifecycleState,
        final_state: Option<LifecycleState>,
        job_id: Option<u32>,
        logical_now_ms: u64,
        failure_phase: Option<LifecycleFailurePhase>,
    ) -> RetainedMutationResult {
        RetainedMutationResult::Lifecycle(LifecycleTerminalResult {
            operation_id: request.operation_id,
            origin_worker_epoch: request.origin_worker_epoch,
            worker_epoch: request.origin_worker_epoch,
            manager_epoch,
            selector: request.selector.clone(),
            lifecycle,
            action: request.action,
            authorization: LifecycleAuthorization::OwnUser,
            disposition,
            outcome,
            initial_state,
            final_state,
            job_id,
            invocation_id: None,
            accepted_ms: logical_now_ms,
            submission_ms: (disposition == LifecycleDisposition::WorkerSubmitted)
                .then_some(logical_now_ms),
            completed_ms: logical_now_ms,
            dependencies_affected: false,
            manifest_changed: false,
            failure_phase,
        })
    }
}

fn reconciliation_terminal(
    record: &InterlockRecord,
    observation: super::lifecycle::LifecycleObservation,
) -> bool {
    if observation.queued_job.is_some() || observation.correlatable_jobless_transition {
        return false;
    }
    match record.phase {
        InterlockPhase::Prepared | InterlockPhase::CommittingCancel => false,
        InterlockPhase::CancelCommittedStopPending => {
            observation.state == LifecycleState::Inactive
                || (observation.state == LifecycleState::Failed && observation.failed_quiescent)
        }
        InterlockPhase::CommittingSubmission
        | InterlockPhase::CommittingStop
        | InterlockPhase::ObservingExisting
        | InterlockPhase::CommittedSubmission => {
            terminal_satisfied(record.action, record.lifecycle, observation)
                || observation.execution_result == CorrelatedResult::Failed
                || observation.stop_result == CorrelatedResult::Failed
        }
    }
}

fn terminal_satisfied(
    action: LifecycleAction,
    lifecycle: WorkloadLifecycle,
    observation: super::lifecycle::LifecycleObservation,
) -> bool {
    match action {
        LifecycleAction::Down => {
            observation.stop_result == CorrelatedResult::Succeeded
                && (observation.state == LifecycleState::Inactive
                    || (observation.state == LifecycleState::Failed
                        && observation.failed_quiescent))
        }
        LifecycleAction::Up => match lifecycle {
            WorkloadLifecycle::LongRunning => {
                matches!(observation.state, LifecycleState::ActiveRunning)
            }
            WorkloadLifecycle::Setup => {
                matches!(observation.state, LifecycleState::ActiveExited)
                    && observation.execution_result == CorrelatedResult::Succeeded
            }
            WorkloadLifecycle::Job => {
                matches!(observation.state, LifecycleState::Inactive)
                    && observation.execution_result == CorrelatedResult::Succeeded
            }
        },
        LifecycleAction::Restart => {
            observation.new_invocation
                && match lifecycle {
                    WorkloadLifecycle::LongRunning => {
                        matches!(observation.state, LifecycleState::ActiveRunning)
                    }
                    WorkloadLifecycle::Setup => {
                        matches!(observation.state, LifecycleState::ActiveExited)
                            && observation.execution_result == CorrelatedResult::Succeeded
                    }
                    WorkloadLifecycle::Job => {
                        matches!(observation.state, LifecycleState::Inactive)
                            && observation.execution_result == CorrelatedResult::Succeeded
                    }
                }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::fs::{self, OpenOptions};
    use std::os::unix::fs::{MetadataExt as _, OpenOptionsExt as _, PermissionsExt as _};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;

    use tempfile::TempDir;

    use crate::protocol::{ManifestGeneration, WorkerTarget};
    use crate::worker::lifecycle::{
        LifecycleObservation, ManagerEpoch, ManagerSubmission, OperationIdentifier,
    };
    use crate::worker::observation::{ObservationText, WorkloadSelector};

    use super::*;

    #[derive(Debug, Clone)]
    struct MockManager {
        observations: Arc<Mutex<VecDeque<LifecycleObservation>>>,
        submission: Result<ManagerSubmission, LifecycleAdapterError>,
        submissions: Arc<AtomicUsize>,
        epoch_calls: Arc<AtomicUsize>,
        change_epoch_at: Arc<AtomicUsize>,
        cancellation: Arc<Mutex<Result<(), LifecycleAdapterError>>>,
    }

    impl LifecycleManagerAdapter for MockManager {
        fn epoch(&self) -> Result<ManagerEpoch, LifecycleAdapterError> {
            let call = self.epoch_calls.fetch_add(1, Ordering::Relaxed);
            if call >= self.change_epoch_at.load(Ordering::Relaxed) {
                let mut changed = manager_epoch();
                changed.bus_id = ObservationText::parse("replacement-bus").unwrap();
                Ok(changed)
            } else {
                Ok(manager_epoch())
            }
        }

        fn observe(
            &self,
            _selector: &BackendSelector,
        ) -> Result<LifecycleObservation, LifecycleAdapterError> {
            self.observations
                .lock()
                .map_err(|_| LifecycleAdapterError::Unavailable)?
                .pop_front()
                .ok_or(LifecycleAdapterError::Unavailable)
        }

        fn cancel_start(
            &self,
            _selector: &BackendSelector,
            _job_id: u32,
        ) -> Result<(), LifecycleAdapterError> {
            self.submissions.fetch_add(1, Ordering::Relaxed);
            *self
                .cancellation
                .lock()
                .map_err(|_| LifecycleAdapterError::Unavailable)?
        }

        fn submit(
            &self,
            _selector: &BackendSelector,
            _action: ManagerAction,
        ) -> Result<ManagerSubmission, LifecycleAdapterError> {
            self.submissions.fetch_add(1, Ordering::Relaxed);
            self.submission.clone()
        }
    }

    fn worker_epoch() -> ConnectionIdentifier {
        ConnectionIdentifier::parse("018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20").unwrap()
    }

    fn manager_epoch() -> ManagerEpoch {
        ManagerEpoch {
            boot_id: ObservationText::parse("boot").unwrap(),
            bus_id: ObservationText::parse("bus").unwrap(),
            systemd_owner: ObservationText::parse(":1.42").unwrap(),
        }
    }

    fn observation(state: LifecycleState) -> LifecycleObservation {
        LifecycleObservation {
            state,
            queued_job: None,
            failed_quiescent: true,
            correlatable_jobless_transition: false,
            execution_result: CorrelatedResult::None,
            new_invocation: false,
            stop_result: CorrelatedResult::None,
        }
    }

    fn request(action: LifecycleAction) -> LifecycleRequest {
        LifecycleRequest {
            operation_id: OperationIdentifier::parse("018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c21")
                .unwrap(),
            origin_worker_epoch: worker_epoch(),
            selector: WorkloadSelector {
                target: WorkerTarget::User,
                name: ObservationText::parse("alpha").unwrap(),
                generation: ManifestGeneration::parse("a".repeat(64)).unwrap(),
                workload_id: ManifestGeneration::parse("b".repeat(64)).unwrap(),
            },
            action,
        }
    }

    fn selector() -> BackendSelector {
        BackendSelector {
            workload_name: ObservationText::parse("alpha").unwrap(),
            generated_service: ObservationText::parse("alpha.service").unwrap(),
            container_name: ObservationText::parse("alpha").unwrap(),
        }
    }

    fn fixture(
        observations: Vec<LifecycleObservation>,
        submission: Result<ManagerSubmission, LifecycleAdapterError>,
    ) -> (TempDir, LifecycleCoordinator<MockManager>, Arc<AtomicUsize>) {
        let temporary = TempDir::new().unwrap();
        let directory = temporary.path().join("interlocks");
        fs::create_dir(&directory).unwrap();
        fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
        let lock = temporary.path().join("activation.lock");
        OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&lock)
            .unwrap();
        let metadata = directory.metadata().unwrap();
        let store = InterlockStore::new(directory, lock, metadata.uid(), metadata.gid()).unwrap();
        let submissions = Arc::new(AtomicUsize::new(0));
        let manager = MockManager {
            observations: Arc::new(Mutex::new(observations.into())),
            submission,
            submissions: submissions.clone(),
            epoch_calls: Arc::new(AtomicUsize::new(0)),
            change_epoch_at: Arc::new(AtomicUsize::new(usize::MAX)),
            cancellation: Arc::new(Mutex::new(Ok(()))),
        };
        (
            temporary,
            LifecycleCoordinator::new(worker_epoch(), manager, store),
            submissions,
        )
    }

    fn retained_outcome(result: &RetainedMutationResult) -> MutationTerminal {
        match result {
            RetainedMutationResult::Lifecycle(result) => result.outcome,
            RetainedMutationResult::BeforeCommitment(_) => {
                MutationTerminal::DepartedBeforeCommitment
            }
        }
    }

    fn terminal_outcome(execution: &LifecycleExecution) -> Option<MutationTerminal> {
        match execution {
            LifecycleExecution::Terminal(result) => Some(retained_outcome(result)),
            _ => None,
        }
    }

    fn query_outcome(query: &MutationQuery) -> Option<MutationTerminal> {
        match query {
            MutationQuery::Terminal(result) => Some(retained_outcome(result)),
            _ => None,
        }
    }

    fn accepted() -> ManagerSubmission {
        ManagerSubmission {
            epoch: manager_epoch(),
            job_id: Some(7),
        }
    }

    #[test]
    fn reconciliation_requires_phase_specific_terminal_evidence() {
        let value = request(LifecycleAction::Up);
        let mut record = InterlockRecord {
            worker_epoch: worker_epoch(),
            principal_uid: 1000,
            operation_id: value.operation_id,
            selector: value.selector,
            lifecycle: WorkloadLifecycle::LongRunning,
            action: LifecycleAction::Up,
            backend_selector: selector(),
            initial_state: LifecycleState::Failed,
            phase: InterlockPhase::CommittingSubmission,
            manager_epoch: Some(manager_epoch()),
            job_id: None,
            invocation_id: None,
        };
        let stale_failed = observation(LifecycleState::Failed);
        assert!(!reconciliation_terminal(&record, stale_failed));

        let mut correlated_failed = stale_failed;
        correlated_failed.execution_result = CorrelatedResult::Failed;
        assert!(reconciliation_terminal(&record, correlated_failed));
        record.phase = InterlockPhase::CommittingCancel;
        assert!(!reconciliation_terminal(&record, correlated_failed));
        record.phase = InterlockPhase::CancelCommittedStopPending;
        let quiescent = observation(LifecycleState::Inactive);
        assert!(reconciliation_terminal(&record, quiescent));
    }

    #[test]
    fn disabled_and_no_change_never_submit_or_create_interlocks() {
        let (_temporary, coordinator, submissions) = fixture(
            vec![observation(LifecycleState::ActiveRunning)],
            Ok(accepted()),
        );
        let value = request(LifecycleAction::Up);
        let now = value.operation_id.timestamp_ms();
        assert_eq!(
            coordinator.execute(
                1000,
                value.clone(),
                &selector(),
                WorkloadLifecycle::LongRunning,
                false,
                now
            ),
            LifecycleExecution::Rejected(LifecycleRejection::WorkloadDisabled)
        );
        assert_eq!(
            terminal_outcome(&coordinator.execute(
                1000,
                value,
                &selector(),
                WorkloadLifecycle::LongRunning,
                true,
                now,
            )),
            Some(MutationTerminal::Succeeded)
        );
        assert_eq!(submissions.load(Ordering::Relaxed), 0);
        assert!(coordinator.interlocks.load().unwrap().is_empty());
    }

    #[test]
    fn final_precommitment_departure_prevents_manager_calls() {
        let (_temporary, coordinator, submissions) =
            fixture(vec![observation(LifecycleState::Inactive)], Ok(accepted()));
        let value = request(LifecycleAction::Up);
        let now = value.operation_id.timestamp_ms();
        let gate = Mutex::new(CommitmentGate::new());
        gate.lock().unwrap().depart(CallerDeparture::Cancelled);

        assert_eq!(
            terminal_outcome(&coordinator.execute_with_gate(
                1000,
                value,
                &selector(),
                WorkloadLifecycle::LongRunning,
                true,
                now,
                &gate,
            )),
            Some(MutationTerminal::DepartedBeforeCommitment)
        );
        assert_eq!(submissions.load(Ordering::Relaxed), 0);
        assert!(coordinator.interlocks.load().unwrap().is_empty());
    }

    #[test]
    fn manager_acceptance_requires_correlated_terminal_evidence() {
        for (action, lifecycle, initial, terminal) in [
            (
                LifecycleAction::Up,
                WorkloadLifecycle::Job,
                LifecycleState::Inactive,
                LifecycleState::Inactive,
            ),
            (
                LifecycleAction::Restart,
                WorkloadLifecycle::LongRunning,
                LifecycleState::ActiveRunning,
                LifecycleState::ActiveRunning,
            ),
        ] {
            let (_temporary, coordinator, submissions) = fixture(
                vec![observation(initial), observation(terminal)],
                Ok(accepted()),
            );
            let value = request(action);
            let now = value.operation_id.timestamp_ms();
            assert_eq!(
                coordinator.execute(1000, value, &selector(), lifecycle, true, now,),
                LifecycleExecution::InProgress(MutationPhase::Observing)
            );
            assert_eq!(submissions.load(Ordering::Relaxed), 1);
            assert_eq!(coordinator.interlocks.load().unwrap().len(), 1);
        }
    }

    #[test]
    fn activating_down_persists_each_cancel_then_stop_commit_phase() {
        let mut activating = observation(LifecycleState::Activating);
        activating.queued_job = Some(super::super::lifecycle::QueuedJob {
            job_id: 7,
            kind: super::super::lifecycle::ManagerJobKind::Start,
            correlated: true,
        });

        let (_temporary, coordinator, _submissions) = fixture(vec![activating], Ok(accepted()));
        *coordinator.manager.cancellation.lock().unwrap() = Err(LifecycleAdapterError::Ambiguous);
        let value = request(LifecycleAction::Down);
        let now = value.operation_id.timestamp_ms();
        assert_eq!(
            terminal_outcome(&coordinator.execute(
                1000,
                value,
                &selector(),
                WorkloadLifecycle::LongRunning,
                true,
                now,
            )),
            Some(MutationTerminal::ResultUnknown)
        );
        assert_eq!(
            coordinator.interlocks.load().unwrap()[0].phase,
            InterlockPhase::CommittingCancel
        );

        let (_temporary, coordinator, _submissions) =
            fixture(vec![activating], Err(LifecycleAdapterError::Ambiguous));
        let value = request(LifecycleAction::Down);
        assert_eq!(
            terminal_outcome(&coordinator.execute(
                1000,
                value,
                &selector(),
                WorkloadLifecycle::LongRunning,
                true,
                now,
            )),
            Some(MutationTerminal::ResultUnknown)
        );
        assert_eq!(
            coordinator.interlocks.load().unwrap()[0].phase,
            InterlockPhase::CommittingStop
        );
    }

    #[test]
    fn down_failed_requires_quiescence_and_correlated_stop_success() {
        for (quiescent, stop_result, succeeds) in [
            (false, CorrelatedResult::Succeeded, false),
            (true, CorrelatedResult::None, false),
            (true, CorrelatedResult::Succeeded, true),
        ] {
            let mut terminal = observation(LifecycleState::Failed);
            terminal.failed_quiescent = quiescent;
            terminal.stop_result = stop_result;
            let (_temporary, coordinator, _submissions) = fixture(
                vec![observation(LifecycleState::ActiveRunning), terminal],
                Ok(accepted()),
            );
            let value = request(LifecycleAction::Down);
            let now = value.operation_id.timestamp_ms();
            let execution = coordinator.execute(
                1000,
                value,
                &selector(),
                WorkloadLifecycle::LongRunning,
                true,
                now,
            );
            if succeeds {
                assert_eq!(
                    terminal_outcome(&execution),
                    Some(MutationTerminal::Succeeded)
                );
            } else {
                assert_eq!(
                    execution,
                    LifecycleExecution::InProgress(MutationPhase::Observing)
                );
            }
        }
    }

    #[test]
    fn manager_epoch_change_cannot_terminalize_or_clear_old_work() {
        let terminal = observation(LifecycleState::ActiveRunning);
        let (_temporary, coordinator, _submissions) = fixture(
            vec![observation(LifecycleState::Inactive), terminal],
            Ok(accepted()),
        );
        coordinator
            .manager
            .change_epoch_at
            .store(1, Ordering::Relaxed);
        let value = request(LifecycleAction::Up);
        let now = value.operation_id.timestamp_ms();

        assert_eq!(
            coordinator.execute(
                1000,
                value,
                &selector(),
                WorkloadLifecycle::LongRunning,
                true,
                now,
            ),
            LifecycleExecution::InProgress(MutationPhase::Observing)
        );
        assert_eq!(coordinator.interlocks.load().unwrap().len(), 1);
    }

    #[test]
    fn proven_success_clears_interlock_while_rejection_and_ambiguity_stay_distinct() {
        let mut terminal = observation(LifecycleState::ActiveRunning);
        terminal.new_invocation = true;
        let (_temporary, coordinator, submissions) = fixture(
            vec![
                observation(LifecycleState::Inactive),
                observation(LifecycleState::Activating),
                terminal,
            ],
            Ok(accepted()),
        );
        let value = request(LifecycleAction::Up);
        let now = value.operation_id.timestamp_ms();
        let execution = coordinator.execute(
            1000,
            value.clone(),
            &selector(),
            WorkloadLifecycle::LongRunning,
            true,
            now,
        );
        assert_eq!(
            terminal_outcome(&execution),
            Some(MutationTerminal::Succeeded)
        );
        assert!(coordinator.interlocks.load().unwrap().is_empty());
        let query = coordinator.query(1000, &value, now);
        assert_eq!(query_outcome(&query), Some(MutationTerminal::Succeeded));
        let MutationQuery::Terminal(query_result) = query else {
            panic!("terminal query expected");
        };
        let LifecycleExecution::Terminal(execution_result) = &execution else {
            panic!("terminal execution expected");
        };
        assert_eq!(&query_result, execution_result);
        assert_eq!(
            coordinator.execute(
                1000,
                value,
                &selector(),
                WorkloadLifecycle::LongRunning,
                true,
                now,
            ),
            execution
        );
        assert_eq!(submissions.load(Ordering::Relaxed), 1);

        for (error, terminal, retained) in [
            (LifecycleAdapterError::Rejected, MutationTerminal::Failed, 0),
            (
                LifecycleAdapterError::Ambiguous,
                MutationTerminal::ResultUnknown,
                1,
            ),
        ] {
            let (_temporary, coordinator, _submissions) =
                fixture(vec![observation(LifecycleState::Inactive)], Err(error));
            let value = request(LifecycleAction::Up);
            let now = value.operation_id.timestamp_ms();
            assert_eq!(
                terminal_outcome(&coordinator.execute(
                    1000,
                    value,
                    &selector(),
                    WorkloadLifecycle::LongRunning,
                    true,
                    now,
                )),
                Some(terminal)
            );
            assert_eq!(coordinator.interlocks.load().unwrap().len(), retained);
            assert_eq!(
                coordinator.reconcile_interlocks().unwrap().retained,
                retained
            );
            if retained == 1 {
                let mut next = request(LifecycleAction::Up);
                next.operation_id =
                    OperationIdentifier::parse("018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c22").unwrap();
                assert_eq!(
                    coordinator.execute(
                        1000,
                        next,
                        &selector(),
                        WorkloadLifecycle::LongRunning,
                        true,
                        now,
                    ),
                    LifecycleExecution::Rejected(LifecycleRejection::OperationInProgress)
                );
            }
        }
    }
}
