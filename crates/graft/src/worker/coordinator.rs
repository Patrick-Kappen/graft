//! Lifecycle commitment coordinator for validated user workloads.

use std::sync::{Mutex, MutexGuard};

use crate::manifest::WorkloadLifecycle;
use crate::protocol::ConnectionIdentifier;

use super::discovery::BackendSelector;
use super::interlock::{InterlockPhase, InterlockRecord, InterlockStore};
use super::lifecycle::{
    decide_lifecycle, LifecycleAction, LifecycleAdapterError, LifecycleDecision,
    LifecycleManagerAdapter, LifecycleRequest, LifecycleState, ManagerAction,
};
use super::mutation::{MutationAdmission, MutationPhase, MutationRegistry, MutationTerminal};

/// Result of one lifecycle submission or duplicate join attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleExecution {
    /// Action-specific terminal result was established.
    Terminal(MutationTerminal),
    /// Identical accepted operation remains in progress.
    InProgress(MutationPhase),
    /// Pre-acceptance request rejection.
    Rejected(LifecycleRejection),
}

/// Closed pre-acceptance lifecycle rejection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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

    /// Executes one already-authorized, manifest-bound lifecycle request.
    ///
    /// Manager acceptance is never treated as terminal success; success requires
    /// a second normalized observation satisfying the action/lifecycle contract.
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
        )
    }

    fn execute_new(
        &self,
        registry: &mut MutexGuard<'_, MutationRegistry>,
        principal_uid: u32,
        request: LifecycleRequest,
        backend_selector: &BackendSelector,
        lifecycle: WorkloadLifecycle,
        logical_now_ms: u64,
    ) -> LifecycleExecution {
        let Ok(_activation) = self.interlocks.lock_submission() else {
            return LifecycleExecution::Rejected(LifecycleRejection::InfrastructureUnavailable);
        };
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
                let _ = registry.terminal(
                    principal_uid,
                    request.operation_id,
                    MutationTerminal::Succeeded,
                    logical_now_ms,
                );
                LifecycleExecution::Terminal(MutationTerminal::Succeeded)
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
            ),
        }
    }

    #[allow(clippy::too_many_arguments, clippy::needless_pass_by_value)]
    fn commit(
        &self,
        registry: &mut MutationRegistry,
        principal_uid: u32,
        request: LifecycleRequest,
        backend_selector: &BackendSelector,
        lifecycle: WorkloadLifecycle,
        logical_now_ms: u64,
        manager_epoch: super::lifecycle::ManagerEpoch,
        manager_action: Option<ManagerAction>,
    ) -> LifecycleExecution {
        let mut interlock = InterlockRecord {
            worker_epoch: request.origin_worker_epoch,
            principal_uid,
            operation_id: request.operation_id,
            selector: request.selector.clone(),
            action: request.action,
            phase: InterlockPhase::Prepared,
            manager_epoch: Some(manager_epoch.clone()),
            job_id: None,
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
        interlock.phase = if manager_action.is_some() {
            InterlockPhase::CommittingSubmission
        } else {
            InterlockPhase::ObservingExisting
        };
        if self.interlocks.persist(&interlock).is_err() {
            return Self::unknown(registry, principal_uid, &request, logical_now_ms);
        }
        let _ = registry.advance(
            principal_uid,
            request.operation_id,
            MutationPhase::Committing,
        );
        if let Some(action) = manager_action {
            match self.manager.submit(backend_selector, action) {
                Ok(submission) if submission.epoch == manager_epoch => {
                    interlock.job_id = submission.job_id;
                    interlock.phase = InterlockPhase::CommittedSubmission;
                    if self.interlocks.persist(&interlock).is_err() {
                        return Self::unknown(registry, principal_uid, &request, logical_now_ms);
                    }
                }
                Err(LifecycleAdapterError::Rejected) => {
                    let _ = self.interlocks.remove(request.operation_id);
                    let _ = registry.terminal(
                        principal_uid,
                        request.operation_id,
                        MutationTerminal::Failed,
                        logical_now_ms,
                    );
                    return LifecycleExecution::Terminal(MutationTerminal::Failed);
                }
                _ => {
                    return Self::unknown(registry, principal_uid, &request, logical_now_ms);
                }
            }
        }
        let _ = registry.advance(
            principal_uid,
            request.operation_id,
            MutationPhase::Observing,
        );
        let terminal = match self.manager.observe(backend_selector) {
            Ok(observation) if terminal_satisfied(request.action, lifecycle, observation) => {
                MutationTerminal::Succeeded
            }
            Ok(observation) if observation.state == LifecycleState::Failed => {
                MutationTerminal::Failed
            }
            _ => MutationTerminal::ResultUnknown,
        };
        if terminal != MutationTerminal::ResultUnknown {
            let _ = self.interlocks.remove(request.operation_id);
        }
        let _ = registry.terminal(
            principal_uid,
            request.operation_id,
            terminal,
            logical_now_ms,
        );
        LifecycleExecution::Terminal(terminal)
    }

    fn unknown(
        registry: &mut MutationRegistry,
        principal_uid: u32,
        request: &LifecycleRequest,
        logical_now_ms: u64,
    ) -> LifecycleExecution {
        let _ = registry.terminal(
            principal_uid,
            request.operation_id,
            MutationTerminal::ResultUnknown,
            logical_now_ms,
        );
        LifecycleExecution::Terminal(MutationTerminal::ResultUnknown)
    }
}

const fn terminal_satisfied(
    action: LifecycleAction,
    lifecycle: WorkloadLifecycle,
    observation: super::lifecycle::LifecycleObservation,
) -> bool {
    match action {
        LifecycleAction::Down => matches!(
            observation.state,
            LifecycleState::Inactive | LifecycleState::Failed
        ),
        LifecycleAction::Up => match lifecycle {
            WorkloadLifecycle::LongRunning => {
                matches!(observation.state, LifecycleState::ActiveRunning)
            }
            WorkloadLifecycle::Setup => {
                matches!(observation.state, LifecycleState::ActiveExited)
                    && observation.correlated_execution_succeeded
            }
            WorkloadLifecycle::Job => {
                matches!(observation.state, LifecycleState::Inactive)
                    && observation.correlated_execution_succeeded
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
                            && observation.correlated_execution_succeeded
                    }
                    WorkloadLifecycle::Job => {
                        matches!(observation.state, LifecycleState::Inactive)
                            && observation.correlated_execution_succeeded
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
    }

    impl LifecycleManagerAdapter for MockManager {
        fn epoch(&self) -> Result<ManagerEpoch, LifecycleAdapterError> {
            Ok(manager_epoch())
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
            correlated_execution_succeeded: false,
            new_invocation: false,
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
        };
        (
            temporary,
            LifecycleCoordinator::new(worker_epoch(), manager, store),
            submissions,
        )
    }

    fn accepted() -> ManagerSubmission {
        ManagerSubmission {
            epoch: manager_epoch(),
            job_id: Some(7),
        }
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
            coordinator.execute(
                1000,
                value,
                &selector(),
                WorkloadLifecycle::LongRunning,
                true,
                now
            ),
            LifecycleExecution::Terminal(MutationTerminal::Succeeded)
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
                coordinator.execute(1000, value, &selector(), lifecycle, true, now),
                LifecycleExecution::Terminal(MutationTerminal::ResultUnknown)
            );
            assert_eq!(submissions.load(Ordering::Relaxed), 1);
            assert_eq!(coordinator.interlocks.load().unwrap().len(), 1);
        }
    }

    #[test]
    fn proven_success_clears_interlock_while_rejection_and_ambiguity_stay_distinct() {
        let mut terminal = observation(LifecycleState::ActiveRunning);
        terminal.new_invocation = true;
        let (_temporary, coordinator, submissions) = fixture(
            vec![observation(LifecycleState::Inactive), terminal],
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
                true,
                now
            ),
            LifecycleExecution::Terminal(MutationTerminal::Succeeded)
        );
        assert!(coordinator.interlocks.load().unwrap().is_empty());
        assert_eq!(
            coordinator.execute(
                1000,
                value,
                &selector(),
                WorkloadLifecycle::LongRunning,
                true,
                now
            ),
            LifecycleExecution::Terminal(MutationTerminal::Succeeded)
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
                coordinator.execute(
                    1000,
                    value,
                    &selector(),
                    WorkloadLifecycle::LongRunning,
                    true,
                    now
                ),
                LifecycleExecution::Terminal(terminal)
            );
            assert_eq!(coordinator.interlocks.load().unwrap().len(), retained);
        }
    }
}
