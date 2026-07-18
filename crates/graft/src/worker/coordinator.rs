//! Lifecycle commitment coordinator for validated user workloads.

use std::sync::Mutex;

use serde::{Deserialize, Serialize};

use crate::manifest::WorkloadLifecycle;
use crate::protocol::ConnectionIdentifier;

use super::discovery::BackendSelector;
use super::interlock::{ActivationGuard, InterlockPhase, InterlockRecord, InterlockStore};
use super::lifecycle::{
    decide_lifecycle, CallerDeparture, CommitmentGate, CommitmentOutcome, CorrelatedResult,
    LifecycleAction, LifecycleAdapterError, LifecycleDecision, LifecycleManagerAdapter,
    LifecycleRequest, LifecycleState, ManagerAction,
};
use super::mutation::{
    LifecycleAuthorization, LifecycleDisposition, LifecycleFailurePhase, LifecycleTerminalResult,
    MutationAdmission, MutationGuidance, MutationPhase, MutationQuery, MutationQueryCode,
    MutationQueryError, MutationRegistry, MutationTerminal, MutationTerminalError,
    RetainedMutationResult,
};

const MAX_TERMINAL_OBSERVATIONS: usize = 16;
const TERMINAL_OBSERVATION_WINDOW_MS: u64 = 10 * 60 * 1_000;

#[derive(Debug, Clone, Copy)]
struct LifecycleTimes {
    accepted: u64,
    submission: Option<u64>,
    completed: u64,
}

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
    worker_epoch: ConnectionIdentifier,
    manager: M,
    interlocks: InterlockStore,
    registry: Mutex<MutationRegistry>,
}

impl<M: LifecycleManagerAdapter> LifecycleCoordinator<M> {
    /// Creates one coordinator for a fixed worker epoch and manager context.
    #[must_use]
    pub fn new(worker_epoch: ConnectionIdentifier, manager: M, interlocks: InterlockStore) -> Self {
        Self {
            worker_epoch,
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
            return MutationQuery::CacheLost(MutationQueryError {
                operation_id: request.operation_id,
                origin_worker_epoch: request.origin_worker_epoch,
                worker_epoch: self.worker_epoch,
                observed_ms: logical_now_ms,
                code: MutationQueryCode::CacheLost,
                guidance: MutationGuidance::Reconcile,
            });
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
        let admission = {
            let Ok(mut registry) = self.registry.lock() else {
                return LifecycleExecution::Rejected(LifecycleRejection::InfrastructureUnavailable);
            };
            registry.admit(principal_uid, request.clone(), logical_now_ms)
        };
        match admission {
            MutationAdmission::JoinedInProgress(MutationPhase::Observing) => {
                return self.resume_observation(principal_uid, &request, logical_now_ms);
            }
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
        let operation_id = request.operation_id;
        let execution = self.execute_new(
            principal_uid,
            request,
            backend_selector,
            lifecycle,
            logical_now_ms,
            commitment,
        );
        if matches!(execution, LifecycleExecution::Rejected(_)) {
            self.reject_accepted(principal_uid, operation_id);
        }
        execution
    }

    #[allow(clippy::too_many_arguments)]
    fn execute_new(
        &self,
        principal_uid: u32,
        request: LifecycleRequest,
        backend_selector: &BackendSelector,
        lifecycle: WorkloadLifecycle,
        logical_now_ms: u64,
        commitment: &Mutex<CommitmentGate>,
    ) -> LifecycleExecution {
        let Ok(activation) = self.interlocks.lock_submission() else {
            return LifecycleExecution::Rejected(LifecycleRejection::InfrastructureUnavailable);
        };
        let Ok(retained) = self.interlocks.load() else {
            return LifecycleExecution::Rejected(LifecycleRejection::InfrastructureUnavailable);
        };
        if retained.iter().any(|record| {
            record.selector == request.selector || record.operation_id == request.operation_id
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
                let terminal = Self::lifecycle_result(
                    &request,
                    lifecycle,
                    Some(manager_epoch),
                    LifecycleDisposition::NoChange,
                    MutationTerminal::Succeeded,
                    observation.state,
                    Some(observation.state),
                    None,
                    LifecycleTimes {
                        accepted: logical_now_ms,
                        submission: None,
                        completed: observation.observed_ms.max(logical_now_ms),
                    },
                    None,
                );
                self.publish_terminal(principal_uid, terminal.clone());
                LifecycleExecution::Terminal(Box::new(terminal))
            }
            LifecycleDecision::Submit(action) => self.commit(
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
                activation,
            ),
            LifecycleDecision::JoinExisting | LifecycleDecision::ObserveExisting => self.commit(
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
                activation,
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
        activation: ActivationGuard,
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
            accepted_ms: logical_now_ms,
            observation_deadline_ms: logical_now_ms.saturating_add(TERMINAL_OBSERVATION_WINDOW_MS),
            submission_ms: None,
            initial_state,
            phase: InterlockPhase::Prepared,
            manager_epoch: Some(manager_epoch.clone()),
            job_id: verified_job_id,
            invocation_id: None,
        };
        if self.interlocks.persist_new(&interlock).is_err() {
            return LifecycleExecution::Rejected(LifecycleRejection::InfrastructureUnavailable);
        }
        interlock.phase = match manager_action {
            Some(ManagerAction::CancelStartThenStop) => InterlockPhase::CommittingCancel,
            Some(_) => InterlockPhase::CommittingSubmission,
            None => InterlockPhase::ObservingExisting,
        };
        if self.interlocks.persist(&interlock).is_err() {
            return self.unknown(
                principal_uid,
                &request,
                &interlock,
                initial_state,
                disposition,
                logical_now_ms,
            );
        }
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
                completed_ms: self.manager.logical_now_ms(),
            });
            self.publish_terminal(principal_uid, terminal.clone());
            return LifecycleExecution::Terminal(Box::new(terminal));
        }
        self.mark_committed(principal_uid, request.operation_id, disposition);
        if manager_action == Some(ManagerAction::CancelStartThenStop) {
            let Some(job_id) = verified_job_id else {
                return self.unknown(
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
                        return self.unknown(
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
                        return self.unknown(
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
                        LifecycleTimes {
                            accepted: interlock.accepted_ms,
                            submission: interlock.submission_ms,
                            completed: self.manager.logical_now_ms(),
                        },
                        Some(LifecycleFailurePhase::Submission),
                    );
                    let _ = self.interlocks.remove(request.operation_id);
                    self.publish_terminal(principal_uid, terminal.clone());
                    return LifecycleExecution::Terminal(Box::new(terminal));
                }
                _ => {
                    return self.unknown(
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
                    interlock.submission_ms = Some(submission.accepted_ms);
                    interlock.phase = InterlockPhase::CommittedSubmission;
                    if self.interlocks.persist(&interlock).is_err() {
                        return self.unknown(
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
                        LifecycleTimes {
                            accepted: interlock.accepted_ms,
                            submission: interlock.submission_ms,
                            completed: self.manager.logical_now_ms(),
                        },
                        Some(LifecycleFailurePhase::Submission),
                    );
                    self.publish_terminal(principal_uid, terminal.clone());
                    return LifecycleExecution::Terminal(Box::new(terminal));
                }
                _ => {
                    return self.unknown(
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
        drop(activation);
        self.advance(
            principal_uid,
            request.operation_id,
            MutationPhase::Observing,
        );
        let mut outcome = MutationTerminal::ResultUnknown;
        let mut final_state = None;
        let mut failure_phase = Some(LifecycleFailurePhase::Observation);
        let mut completed_ms = logical_now_ms;
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
            completed_ms = observation.observed_ms.max(logical_now_ms);
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
            if terminal_satisfied(request.action, lifecycle, observation) {
                outcome = MutationTerminal::Succeeded;
                failure_phase = None;
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
            LifecycleTimes {
                accepted: interlock.accepted_ms,
                submission: interlock.submission_ms,
                completed: completed_ms,
            },
            failure_phase,
        );
        self.publish_terminal(principal_uid, terminal.clone());
        LifecycleExecution::Terminal(Box::new(terminal))
    }

    fn resume_observation(
        &self,
        principal_uid: u32,
        request: &LifecycleRequest,
        logical_now_ms: u64,
    ) -> LifecycleExecution {
        let Ok(records) = self.interlocks.load() else {
            return LifecycleExecution::InProgress(MutationPhase::Observing);
        };
        let Some(record) = records.into_iter().find(|record| {
            record.principal_uid == principal_uid
                && record.operation_id == request.operation_id
                && record.selector == request.selector
                && record.action == request.action
        }) else {
            return LifecycleExecution::InProgress(MutationPhase::Observing);
        };
        let Some(manager_epoch) = record.manager_epoch.clone() else {
            return self.unknown(
                principal_uid,
                request,
                &record,
                record.initial_state,
                lifecycle_disposition(record.phase),
                logical_now_ms,
            );
        };
        for _ in 0..MAX_TERMINAL_OBSERVATIONS {
            let (Ok(observation), Ok(observed_epoch)) = (
                self.manager.observe(&record.backend_selector),
                self.manager.epoch(),
            ) else {
                break;
            };
            if observed_epoch != manager_epoch {
                break;
            }
            let failure_phase = if observation.stop_result == CorrelatedResult::Failed {
                Some(LifecycleFailurePhase::Stop)
            } else if observation.execution_result == CorrelatedResult::Failed {
                Some(LifecycleFailurePhase::Execution)
            } else {
                None
            };
            let outcome = if failure_phase.is_some() {
                Some(MutationTerminal::Failed)
            } else if terminal_satisfied(request.action, record.lifecycle, observation) {
                Some(MutationTerminal::Succeeded)
            } else {
                None
            };
            if let Some(outcome) = outcome {
                let _ = self.interlocks.remove(request.operation_id);
                let terminal = Self::lifecycle_result(
                    request,
                    record.lifecycle,
                    Some(manager_epoch),
                    lifecycle_disposition(record.phase),
                    outcome,
                    record.initial_state,
                    Some(observation.state),
                    record.job_id,
                    LifecycleTimes {
                        accepted: record.accepted_ms,
                        submission: record.submission_ms,
                        completed: observation.observed_ms.max(logical_now_ms),
                    },
                    failure_phase,
                );
                self.publish_terminal(principal_uid, terminal.clone());
                return LifecycleExecution::Terminal(Box::new(terminal));
            }
        }
        if logical_now_ms >= record.observation_deadline_ms {
            return self.unknown(
                principal_uid,
                request,
                &record,
                record.initial_state,
                lifecycle_disposition(record.phase),
                logical_now_ms,
            );
        }
        LifecycleExecution::InProgress(MutationPhase::Observing)
    }

    fn reject_accepted(
        &self,
        principal_uid: u32,
        operation_id: super::lifecycle::OperationIdentifier,
    ) {
        if let Ok(mut registry) = self.registry.lock() {
            let _ = registry.reject_accepted(principal_uid, operation_id);
        }
    }

    fn mark_committed(
        &self,
        principal_uid: u32,
        operation_id: super::lifecycle::OperationIdentifier,
        disposition: LifecycleDisposition,
    ) {
        if let Ok(mut registry) = self.registry.lock() {
            let _ = registry.commit(principal_uid, operation_id, disposition);
        }
    }

    fn advance(
        &self,
        principal_uid: u32,
        operation_id: super::lifecycle::OperationIdentifier,
        phase: MutationPhase,
    ) {
        if let Ok(mut registry) = self.registry.lock() {
            let _ = registry.advance(principal_uid, operation_id, phase);
        }
    }

    fn publish_terminal(&self, principal_uid: u32, result: RetainedMutationResult) {
        let (operation_id, completed_ms) = match &result {
            RetainedMutationResult::Lifecycle(value) => (value.operation_id, value.completed_ms),
            RetainedMutationResult::BeforeCommitment(value) => {
                (value.operation_id, value.completed_ms)
            }
        };
        if let Ok(mut registry) = self.registry.lock() {
            let _ = registry.terminal(principal_uid, operation_id, result, completed_ms);
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn unknown(
        &self,
        principal_uid: u32,
        request: &LifecycleRequest,
        interlock: &InterlockRecord,
        initial_state: LifecycleState,
        disposition: LifecycleDisposition,
        _logical_now_ms: u64,
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
            LifecycleTimes {
                accepted: interlock.accepted_ms,
                submission: interlock.submission_ms,
                completed: self.manager.logical_now_ms(),
            },
            Some(LifecycleFailurePhase::Observation),
        );
        self.publish_terminal(principal_uid, terminal.clone());
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
        times: LifecycleTimes,
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
            accepted_ms: times.accepted,
            submission_ms: times.submission,
            completed_ms: times.completed,
            dependencies_affected: false,
            manifest_changed: false,
            failure_phase,
        })
    }
}

const fn lifecycle_disposition(phase: InterlockPhase) -> LifecycleDisposition {
    match phase {
        InterlockPhase::ObservingExisting => LifecycleDisposition::ExistingManagerWork,
        InterlockPhase::Prepared
        | InterlockPhase::CommittingSubmission
        | InterlockPhase::CommittingCancel
        | InterlockPhase::CancelCommittedStopPending
        | InterlockPhase::CommittingStop
        | InterlockPhase::CommittedSubmission => LifecycleDisposition::WorkerSubmitted,
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
        logical_clock: Arc<AtomicUsize>,
    }

    impl LifecycleManagerAdapter for MockManager {
        fn logical_now_ms(&self) -> u64 {
            u64::try_from(self.logical_clock.fetch_add(1, Ordering::Relaxed)).unwrap_or(u64::MAX)
        }

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
            let mut observation = self
                .observations
                .lock()
                .map_err(|_| LifecycleAdapterError::Unavailable)?
                .pop_front()
                .ok_or(LifecycleAdapterError::Unavailable)?;
            observation.observed_ms = self.logical_now_ms();
            Ok(observation)
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
            let mut submission = self.submission.clone()?;
            submission.accepted_ms = self.logical_now_ms();
            Ok(submission)
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
            observed_ms: 0,
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
            logical_clock: Arc::new(AtomicUsize::new(
                usize::try_from(request(LifecycleAction::Up).operation_id.timestamp_ms()).unwrap(),
            )),
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
            accepted_ms: 1,
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
            accepted_ms: value.operation_id.timestamp_ms(),
            observation_deadline_ms: value
                .operation_id
                .timestamp_ms()
                .saturating_add(TERMINAL_OBSERVATION_WINDOW_MS),
            submission_ms: None,
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

        let (_temporary, coordinator, _submissions) = fixture(vec![activating], Ok(accepted()));
        *coordinator.manager.cancellation.lock().unwrap() = Err(LifecycleAdapterError::Rejected);
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
            Some(MutationTerminal::Failed)
        );
        assert!(coordinator.interlocks.load().unwrap().is_empty());
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
    fn restart_rejects_same_identifier_without_replacing_ambiguous_interlock() {
        let (temporary, coordinator, _submissions) = fixture(
            vec![observation(LifecycleState::Inactive)],
            Err(LifecycleAdapterError::Ambiguous),
        );
        let value = request(LifecycleAction::Up);
        let now = value.operation_id.timestamp_ms();
        assert_eq!(
            terminal_outcome(&coordinator.execute(
                1000,
                value.clone(),
                &selector(),
                WorkloadLifecycle::LongRunning,
                true,
                now,
            )),
            Some(MutationTerminal::ResultUnknown)
        );
        let directory = temporary.path().join("interlocks");
        let metadata = directory.metadata().unwrap();
        let store = InterlockStore::new(
            directory,
            temporary.path().join("activation.lock"),
            metadata.uid(),
            metadata.gid(),
        )
        .unwrap();
        let restarted =
            LifecycleCoordinator::new(worker_epoch(), coordinator.manager.clone(), store);

        assert_eq!(
            restarted.execute(
                1000,
                value,
                &selector(),
                WorkloadLifecycle::LongRunning,
                true,
                now,
            ),
            LifecycleExecution::Rejected(LifecycleRejection::OperationInProgress)
        );
        assert_eq!(restarted.interlocks.load().unwrap().len(), 1);
    }

    #[test]
    fn duplicate_join_resumes_observation_and_enforces_absolute_cutoff() {
        let mut observations = vec![observation(LifecycleState::Inactive)];
        observations.extend(
            std::iter::repeat(observation(LifecycleState::Activating))
                .take(MAX_TERMINAL_OBSERVATIONS),
        );
        let mut terminal = observation(LifecycleState::ActiveRunning);
        terminal.observed_ms = 3;
        observations.push(terminal);
        let (_temporary, coordinator, _submissions) = fixture(observations, Ok(accepted()));
        let value = request(LifecycleAction::Up);
        let now = value.operation_id.timestamp_ms();

        assert_eq!(
            coordinator.execute(
                1000,
                value.clone(),
                &selector(),
                WorkloadLifecycle::LongRunning,
                true,
                now,
            ),
            LifecycleExecution::InProgress(MutationPhase::Observing)
        );
        assert_eq!(
            terminal_outcome(&coordinator.execute(
                1000,
                value,
                &selector(),
                WorkloadLifecycle::LongRunning,
                true,
                now + 1,
            )),
            Some(MutationTerminal::Succeeded)
        );

        let mut observations = vec![observation(LifecycleState::Inactive)];
        observations.extend(
            std::iter::repeat(observation(LifecycleState::Activating))
                .take(MAX_TERMINAL_OBSERVATIONS),
        );
        let (_temporary, coordinator, _submissions) = fixture(observations, Ok(accepted()));
        let value = request(LifecycleAction::Up);
        assert_eq!(
            coordinator.execute(
                1000,
                value.clone(),
                &selector(),
                WorkloadLifecycle::LongRunning,
                true,
                now,
            ),
            LifecycleExecution::InProgress(MutationPhase::Observing)
        );
        assert_eq!(
            terminal_outcome(&coordinator.execute(
                1000,
                value,
                &selector(),
                WorkloadLifecycle::LongRunning,
                true,
                now + TERMINAL_OBSERVATION_WINDOW_MS,
            )),
            Some(MutationTerminal::ResultUnknown)
        );
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
