//! Typed lifecycle decisions, mutation identity, and manager adapter boundary.

use serde::{Deserialize, Deserializer, Serialize};
use uuid::Uuid;

use crate::manifest::WorkloadLifecycle;
use crate::protocol::ConnectionIdentifier;

use super::discovery::BackendSelector;
use super::observation::WorkloadSelector;

/// Oldest accepted operation timestamp relative to logical receive time.
pub const OPERATION_PAST_WINDOW_MS: u64 = 10 * 60 * 1_000;
/// Furthest accepted future operation timestamp relative to logical receive time.
pub const OPERATION_FUTURE_WINDOW_MS: u64 = 60 * 1_000;

/// Canonical `UUIDv7` lifecycle operation identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct OperationIdentifier(Uuid);

impl OperationIdentifier {
    /// Parses a canonical lowercase hyphenated `UUIDv7`.
    ///
    /// # Errors
    ///
    /// Returns an error for any non-canonical or non-v7 value.
    pub fn parse(value: &str) -> Result<Self, &'static str> {
        let parsed = Uuid::parse_str(value).map_err(|_| "invalid operation identifier")?;
        if parsed.get_version_num() != 7
            || parsed.get_variant() != uuid::Variant::RFC4122
            || parsed.hyphenated().to_string() != value
        {
            return Err("invalid operation identifier");
        }
        Ok(Self(parsed))
    }

    /// Returns the embedded Unix timestamp in milliseconds.
    #[must_use]
    pub fn timestamp_ms(self) -> u64 {
        let bytes = self.0.as_bytes();
        (u64::from(bytes[0]) << 40)
            | (u64::from(bytes[1]) << 32)
            | (u64::from(bytes[2]) << 24)
            | (u64::from(bytes[3]) << 16)
            | (u64::from(bytes[4]) << 8)
            | u64::from(bytes[5])
    }

    /// Validates the exact half-open replay window.
    #[must_use]
    pub fn is_fresh_at(self, logical_now_ms: u64) -> bool {
        self.timestamp_ms() > logical_now_ms.saturating_sub(OPERATION_PAST_WINDOW_MS)
            && self.timestamp_ms() <= logical_now_ms.saturating_add(OPERATION_FUTURE_WINDOW_MS)
    }

    /// Returns the canonical text.
    #[must_use]
    pub fn to_canonical_string(self) -> String {
        self.0.hyphenated().to_string()
    }
}

impl Serialize for OperationIdentifier {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.hyphenated().to_string())
    }
}

impl<'de> Deserialize<'de> for OperationIdentifier {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(&value).map_err(serde::de::Error::custom)
    }
}

/// Only supported lifecycle mutations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleAction {
    /// Start or converge upward.
    Up,
    /// Stop to a quiescent state.
    Down,
    /// Request one manager restart operation.
    Restart,
}

/// Immutable mutation payload used for deduplication.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct LifecycleRequest {
    /// Principal-scoped operation identity.
    pub operation_id: OperationIdentifier,
    /// Epoch in which the operation must be accepted.
    pub origin_worker_epoch: ConnectionIdentifier,
    /// Exact manifest-bound workload selector.
    pub selector: WorkloadSelector,
    /// Requested typed action.
    pub action: LifecycleAction,
}

/// Normalized lifecycle state read from systemd.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LifecycleState {
    /// Unit is quiescent.
    Inactive,
    /// Correlatable activation is in progress.
    Activating,
    /// Expected running service state.
    ActiveRunning,
    /// Expected retained oneshot state.
    ActiveExited,
    /// Correlatable stop or cleanup is in progress.
    Deactivating,
    /// Unit is failed.
    Failed,
    /// Reload, refresh, or maintenance blocks mutation.
    ManagerBusy,
    /// Transition lacks safe correlation evidence.
    ManagerTransitionConflict,
    /// Manager state or substate is unsupported.
    UnsupportedManagerState,
    /// Authoritative manager state is unavailable.
    Unknown,
}

/// Manager operation represented by a queued job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagerJobKind {
    /// Start job.
    Start,
    /// Stop job.
    Stop,
    /// Restart job.
    Restart,
    /// Any incompatible manager job.
    Other,
}

/// Verified selected-service queued job.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct QueuedJob {
    /// Manager job identity within the captured manager epoch.
    pub job_id: u32,
    /// Normalized manager job kind.
    pub kind: ManagerJobKind,
    /// Whether unit, job ID, and manager epoch correlation are proven.
    pub correlated: bool,
}

/// Operation-correlated execution or stop result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CorrelatedResult {
    /// No terminal correlated result is available.
    None,
    /// Correlated manager work succeeded.
    Succeeded,
    /// Correlated manager work failed.
    Failed,
}

/// Complete normalized manager input to one lifecycle decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct LifecycleObservation {
    /// Logical time at which this evidence was captured.
    pub observed_ms: u64,
    /// Current normalized state.
    pub state: LifecycleState,
    /// Selected-unit queued job, if any.
    pub queued_job: Option<QueuedJob>,
    /// Whether a failed unit is proven quiescent.
    pub failed_quiescent: bool,
    /// Whether a jobless automatic restart or cleanup is safely correlatable.
    pub correlatable_jobless_transition: bool,
    /// Operation-correlated process/invocation result.
    pub execution_result: CorrelatedResult,
    /// Whether a new invocation relative to pre-submission state was proven.
    pub new_invocation: bool,
    /// Operation-correlated stop and cleanup result.
    pub stop_result: CorrelatedResult,
}

/// Manager action selected by the worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagerAction {
    /// Start with conflict-preserving mode.
    Start,
    /// Stop with conflict-preserving mode.
    Stop,
    /// One manager restart call.
    Restart,
    /// Cancel a verified start job then submit stop.
    CancelStartThenStop,
}

/// Safe first-step lifecycle decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "decision", content = "value", rename_all = "snake_case")]
pub enum LifecycleDecision {
    /// State already satisfies the request and no job can reverse it.
    NoChange,
    /// Submit exactly this manager action.
    Submit(ManagerAction),
    /// Join verified compatible manager work.
    JoinExisting,
    /// Observe a recognized jobless transition.
    ObserveExisting,
    /// Existing or transitioning work conflicts.
    Conflict,
    /// Unit shape is incompatible with declared lifecycle.
    UnexpectedState,
    /// Backend state is unavailable.
    BackendUnavailable,
}

/// Applies global state/job rules and the approved action/lifecycle matrices.
#[must_use]
pub fn decide_lifecycle(
    action: LifecycleAction,
    lifecycle: WorkloadLifecycle,
    observation: LifecycleObservation,
) -> LifecycleDecision {
    match observation.state {
        LifecycleState::ManagerBusy | LifecycleState::ManagerTransitionConflict => {
            return LifecycleDecision::Conflict;
        }
        LifecycleState::UnsupportedManagerState => return LifecycleDecision::UnexpectedState,
        LifecycleState::Unknown => return LifecycleDecision::BackendUnavailable,
        _ => {}
    }

    if let Some(job) = observation.queued_job {
        if !job.correlated {
            return LifecycleDecision::Conflict;
        }
        let compatible = matches!(
            (action, job.kind),
            (LifecycleAction::Up, ManagerJobKind::Start)
                | (LifecycleAction::Down, ManagerJobKind::Stop)
                | (LifecycleAction::Restart, ManagerJobKind::Restart)
        );
        if compatible {
            return LifecycleDecision::JoinExisting;
        }
        if action == LifecycleAction::Down && job.kind == ManagerJobKind::Start {
            return LifecycleDecision::Submit(ManagerAction::CancelStartThenStop);
        }
        return LifecycleDecision::Conflict;
    }

    match action {
        LifecycleAction::Up => decide_up(lifecycle, observation),
        LifecycleAction::Down => decide_down(observation),
        LifecycleAction::Restart => decide_restart(lifecycle, observation),
    }
}

const fn decide_up(
    lifecycle: WorkloadLifecycle,
    observation: LifecycleObservation,
) -> LifecycleDecision {
    match observation.state {
        LifecycleState::Inactive | LifecycleState::Failed => {
            LifecycleDecision::Submit(ManagerAction::Start)
        }
        LifecycleState::Activating if observation.correlatable_jobless_transition => {
            LifecycleDecision::ObserveExisting
        }
        LifecycleState::Activating | LifecycleState::Deactivating => LifecycleDecision::Conflict,
        LifecycleState::ActiveRunning if matches!(lifecycle, WorkloadLifecycle::LongRunning) => {
            LifecycleDecision::NoChange
        }
        LifecycleState::ActiveExited if matches!(lifecycle, WorkloadLifecycle::Setup) => {
            LifecycleDecision::NoChange
        }
        LifecycleState::ActiveRunning | LifecycleState::ActiveExited => {
            LifecycleDecision::UnexpectedState
        }
        _ => LifecycleDecision::UnexpectedState,
    }
}

const fn decide_down(observation: LifecycleObservation) -> LifecycleDecision {
    match observation.state {
        LifecycleState::Inactive => LifecycleDecision::NoChange,
        LifecycleState::Failed if observation.failed_quiescent => LifecycleDecision::NoChange,
        LifecycleState::Failed | LifecycleState::ActiveRunning | LifecycleState::ActiveExited => {
            LifecycleDecision::Submit(ManagerAction::Stop)
        }
        LifecycleState::Activating if observation.correlatable_jobless_transition => {
            LifecycleDecision::Submit(ManagerAction::Stop)
        }
        LifecycleState::Deactivating if observation.correlatable_jobless_transition => {
            LifecycleDecision::ObserveExisting
        }
        LifecycleState::Activating | LifecycleState::Deactivating => LifecycleDecision::Conflict,
        _ => LifecycleDecision::UnexpectedState,
    }
}

const fn decide_restart(
    lifecycle: WorkloadLifecycle,
    observation: LifecycleObservation,
) -> LifecycleDecision {
    match observation.state {
        LifecycleState::Inactive | LifecycleState::Failed => {
            LifecycleDecision::Submit(ManagerAction::Restart)
        }
        LifecycleState::ActiveRunning if matches!(lifecycle, WorkloadLifecycle::LongRunning) => {
            LifecycleDecision::Submit(ManagerAction::Restart)
        }
        LifecycleState::ActiveExited if matches!(lifecycle, WorkloadLifecycle::Setup) => {
            LifecycleDecision::Submit(ManagerAction::Restart)
        }
        LifecycleState::Activating | LifecycleState::Deactivating => LifecycleDecision::Conflict,
        LifecycleState::ActiveRunning | LifecycleState::ActiveExited => {
            LifecycleDecision::UnexpectedState
        }
        _ => LifecycleDecision::UnexpectedState,
    }
}

/// Why the final interested caller departed before commitment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CallerDeparture {
    /// Explicit protocol cancellation.
    Cancelled,
    /// Caller deadline elapsed.
    Deadline,
    /// Connection disappeared.
    Disconnected,
}

/// Linearized manager-work commitment outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitmentOutcome {
    /// Commitment won; later departures cannot reverse manager work.
    Committed,
    /// Final caller departed first; no attachment or backend call is permitted.
    Departed(CallerDeparture),
}

/// Request-local interest gate serialized with manager-work commitment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommitmentGate {
    callers: u32,
    committed: bool,
    final_departure: Option<CallerDeparture>,
}

impl CommitmentGate {
    /// Creates a gate with the originating caller interested.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            callers: 1,
            committed: false,
            final_departure: None,
        }
    }

    /// Adds one duplicate caller before or after commitment.
    #[must_use]
    pub fn join(&mut self) -> bool {
        let Some(callers) = self.callers.checked_add(1) else {
            return false;
        };
        self.callers = callers;
        true
    }

    /// Removes one caller and records only the final pre-commitment departure.
    pub fn depart(&mut self, reason: CallerDeparture) {
        self.callers = self.callers.saturating_sub(1);
        if self.callers == 0 && !self.committed && self.final_departure.is_none() {
            self.final_departure = Some(reason);
        }
    }

    /// Attempts the single manager-work commitment linearization point.
    #[must_use]
    pub fn commit(&mut self) -> CommitmentOutcome {
        if let Some(reason) = self.final_departure {
            return CommitmentOutcome::Departed(reason);
        }
        self.committed = true;
        CommitmentOutcome::Committed
    }

    /// Returns whether manager work has committed.
    #[must_use]
    pub const fn is_committed(&self) -> bool {
        self.committed
    }
}

impl Default for CommitmentGate {
    fn default() -> Self {
        Self::new()
    }
}

/// Stable manager epoch; job identities never cross this boundary.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ManagerEpoch {
    /// Bounded host boot identity captured from the fixed host.
    pub boot_id: super::observation::ObservationText,
    /// Bounded D-Bus bus identity captured from the fixed connection.
    pub bus_id: super::observation::ObservationText,
    /// Bounded systemd unique owner.
    pub systemd_owner: super::observation::ObservationText,
}

/// Narrow lifecycle manager adapter with no Podman fallback.
pub trait LifecycleManagerAdapter: Send + Sync + std::fmt::Debug {
    /// Captures current worker logical time at an adapter boundary.
    fn logical_now_ms(&self) -> u64;

    /// Captures the fixed manager epoch.
    ///
    /// # Errors
    ///
    /// Returns a typed error when the manager or its identity is unavailable.
    fn epoch(&self) -> Result<ManagerEpoch, LifecycleAdapterError>;
    /// Reads normalized state for one worker-derived selector.
    ///
    /// # Errors
    ///
    /// Returns a typed error when authoritative state cannot be obtained.
    fn observe(
        &self,
        selector: &BackendSelector,
    ) -> Result<LifecycleObservation, LifecycleAdapterError>;
    /// Cancels one previously verified selected-service start job.
    ///
    /// # Errors
    ///
    /// Returns a typed rejection, ambiguity, epoch-change, or availability error.
    fn cancel_start(
        &self,
        selector: &BackendSelector,
        job_id: u32,
    ) -> Result<(), LifecycleAdapterError>;

    /// Commits one worker-selected manager operation.
    ///
    /// # Errors
    ///
    /// Returns a typed rejection, ambiguity, epoch-change, or availability error.
    fn submit(
        &self,
        selector: &BackendSelector,
        action: ManagerAction,
    ) -> Result<ManagerSubmission, LifecycleAdapterError>;
}

/// Typed manager submission acknowledgement; not lifecycle success.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerSubmission {
    /// Logical time at which manager acceptance was captured.
    pub accepted_ms: u64,
    /// Manager epoch that accepted the call.
    pub epoch: ManagerEpoch,
    /// Bounded manager job identity when returned.
    pub job_id: Option<u32>,
}

/// Closed adapter failure without raw D-Bus payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifecycleAdapterError {
    /// Fixed manager cannot be reached.
    Unavailable,
    /// Manager epoch changed during the operation.
    EpochChanged,
    /// Manager rejected the exact operation.
    Rejected,
    /// Submission result cannot be proven.
    Ambiguous,
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn lifecycle_wire_types_round_trip_and_reject_unknown_fields() {
        let request = LifecycleRequest {
            operation_id: OperationIdentifier::parse("018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c21")
                .unwrap(),
            origin_worker_epoch: ConnectionIdentifier::parse(
                "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20",
            )
            .unwrap(),
            selector: WorkloadSelector {
                target: crate::protocol::WorkerTarget::User,
                name: super::super::observation::ObservationText::parse("alpha").unwrap(),
                generation: crate::protocol::ManifestGeneration::parse("a".repeat(64)).unwrap(),
                workload_id: crate::protocol::ManifestGeneration::parse("b".repeat(64)).unwrap(),
            },
            action: LifecycleAction::Restart,
        };
        let encoded = serde_json::to_vec(&request).unwrap();
        assert_eq!(
            serde_json::from_slice::<LifecycleRequest>(&encoded).unwrap(),
            request
        );
        let mut value = serde_json::to_value(&request).unwrap();
        value
            .as_object_mut()
            .unwrap()
            .insert("raw_unit".into(), serde_json::json!("foreign.service"));
        assert!(serde_json::from_value::<LifecycleRequest>(value).is_err());
    }

    #[test]
    fn uuidv7_replay_window_uses_exact_open_past_and_closed_future_boundaries() {
        fn id(timestamp_ms: u64) -> OperationIdentifier {
            let mut bytes = *Uuid::nil().as_bytes();
            bytes[0..6].copy_from_slice(&timestamp_ms.to_be_bytes()[2..8]);
            bytes[6] = 0x70;
            bytes[8] = 0x80;
            OperationIdentifier(Uuid::from_bytes(bytes))
        }
        let now = 1_800_000_000_000;

        assert!(!id(now - OPERATION_PAST_WINDOW_MS).is_fresh_at(now));
        assert!(id(now - OPERATION_PAST_WINDOW_MS + 1).is_fresh_at(now));
        assert!(id(now + OPERATION_FUTURE_WINDOW_MS).is_fresh_at(now));
        assert!(!id(now + OPERATION_FUTURE_WINDOW_MS + 1).is_fresh_at(now));
        assert!(OperationIdentifier::parse(&id(now).to_canonical_string()).is_ok());
        assert!(OperationIdentifier::parse("00000000-0000-0000-0000-000000000000").is_err());
    }

    fn expected_decision(
        action: LifecycleAction,
        lifecycle: WorkloadLifecycle,
        state: LifecycleState,
    ) -> LifecycleDecision {
        match state {
            LifecycleState::ManagerBusy | LifecycleState::ManagerTransitionConflict => {
                LifecycleDecision::Conflict
            }
            LifecycleState::UnsupportedManagerState => LifecycleDecision::UnexpectedState,
            LifecycleState::Unknown => LifecycleDecision::BackendUnavailable,
            _ => match action {
                LifecycleAction::Down => match state {
                    LifecycleState::Inactive | LifecycleState::Failed => {
                        LifecycleDecision::NoChange
                    }
                    LifecycleState::ActiveRunning | LifecycleState::ActiveExited => {
                        LifecycleDecision::Submit(ManagerAction::Stop)
                    }
                    LifecycleState::Activating | LifecycleState::Deactivating => {
                        LifecycleDecision::Conflict
                    }
                    _ => unreachable!(),
                },
                LifecycleAction::Up => match state {
                    LifecycleState::Inactive | LifecycleState::Failed => {
                        LifecycleDecision::Submit(ManagerAction::Start)
                    }
                    LifecycleState::Activating | LifecycleState::Deactivating => {
                        LifecycleDecision::Conflict
                    }
                    LifecycleState::ActiveRunning
                        if lifecycle == WorkloadLifecycle::LongRunning =>
                    {
                        LifecycleDecision::NoChange
                    }
                    LifecycleState::ActiveExited if lifecycle == WorkloadLifecycle::Setup => {
                        LifecycleDecision::NoChange
                    }
                    LifecycleState::ActiveRunning | LifecycleState::ActiveExited => {
                        LifecycleDecision::UnexpectedState
                    }
                    _ => unreachable!(),
                },
                LifecycleAction::Restart => match state {
                    LifecycleState::Inactive | LifecycleState::Failed => {
                        LifecycleDecision::Submit(ManagerAction::Restart)
                    }
                    LifecycleState::Activating | LifecycleState::Deactivating => {
                        LifecycleDecision::Conflict
                    }
                    LifecycleState::ActiveRunning
                        if lifecycle == WorkloadLifecycle::LongRunning =>
                    {
                        LifecycleDecision::Submit(ManagerAction::Restart)
                    }
                    LifecycleState::ActiveExited if lifecycle == WorkloadLifecycle::Setup => {
                        LifecycleDecision::Submit(ManagerAction::Restart)
                    }
                    LifecycleState::ActiveRunning | LifecycleState::ActiveExited => {
                        LifecycleDecision::UnexpectedState
                    }
                    _ => unreachable!(),
                },
            },
        }
    }

    #[test]
    fn lifecycle_matrix_covers_every_action_lifecycle_and_stable_state() {
        for lifecycle in [
            WorkloadLifecycle::LongRunning,
            WorkloadLifecycle::Setup,
            WorkloadLifecycle::Job,
        ] {
            for action in [
                LifecycleAction::Up,
                LifecycleAction::Down,
                LifecycleAction::Restart,
            ] {
                for state in [
                    LifecycleState::Inactive,
                    LifecycleState::Activating,
                    LifecycleState::ActiveRunning,
                    LifecycleState::ActiveExited,
                    LifecycleState::Deactivating,
                    LifecycleState::Failed,
                    LifecycleState::ManagerBusy,
                    LifecycleState::ManagerTransitionConflict,
                    LifecycleState::UnsupportedManagerState,
                    LifecycleState::Unknown,
                ] {
                    assert_eq!(
                        decide_lifecycle(action, lifecycle, observation(state)),
                        expected_decision(action, lifecycle, state),
                        "wrong matrix row: {action:?}/{lifecycle:?}/{state:?}"
                    );
                }
            }
        }
    }

    #[test]
    fn every_queued_job_action_and_correlation_row_is_explicit() {
        for action in [
            LifecycleAction::Up,
            LifecycleAction::Down,
            LifecycleAction::Restart,
        ] {
            for kind in [
                ManagerJobKind::Start,
                ManagerJobKind::Stop,
                ManagerJobKind::Restart,
                ManagerJobKind::Other,
            ] {
                for correlated in [false, true] {
                    let mut value = observation(LifecycleState::Activating);
                    value.queued_job = Some(QueuedJob {
                        job_id: 7,
                        kind,
                        correlated,
                    });
                    let expected = if correlated {
                        match (action, kind) {
                            (LifecycleAction::Up, ManagerJobKind::Start)
                            | (LifecycleAction::Down, ManagerJobKind::Stop)
                            | (LifecycleAction::Restart, ManagerJobKind::Restart) => {
                                LifecycleDecision::JoinExisting
                            }
                            (LifecycleAction::Down, ManagerJobKind::Start) => {
                                LifecycleDecision::Submit(ManagerAction::CancelStartThenStop)
                            }
                            _ => LifecycleDecision::Conflict,
                        }
                    } else {
                        LifecycleDecision::Conflict
                    };
                    assert_eq!(
                        decide_lifecycle(action, WorkloadLifecycle::LongRunning, value),
                        expected,
                        "wrong queued-job row: {action:?}/{kind:?}/{correlated}"
                    );
                }
            }
        }
    }

    #[test]
    fn global_and_queued_job_rules_precede_action_matrices() {
        for state in [
            LifecycleState::ManagerBusy,
            LifecycleState::ManagerTransitionConflict,
        ] {
            assert_eq!(
                decide_lifecycle(
                    LifecycleAction::Up,
                    WorkloadLifecycle::LongRunning,
                    observation(state),
                ),
                LifecycleDecision::Conflict
            );
        }
        assert_eq!(
            decide_lifecycle(
                LifecycleAction::Up,
                WorkloadLifecycle::LongRunning,
                observation(LifecycleState::Unknown),
            ),
            LifecycleDecision::BackendUnavailable
        );
        let mut value = observation(LifecycleState::Activating);
        value.queued_job = Some(QueuedJob {
            job_id: 7,
            kind: ManagerJobKind::Start,
            correlated: true,
        });
        assert_eq!(
            decide_lifecycle(LifecycleAction::Up, WorkloadLifecycle::LongRunning, value,),
            LifecycleDecision::JoinExisting
        );
        assert_eq!(
            decide_lifecycle(LifecycleAction::Down, WorkloadLifecycle::Job, value),
            LifecycleDecision::Submit(ManagerAction::CancelStartThenStop)
        );
        assert_eq!(
            decide_lifecycle(LifecycleAction::Restart, WorkloadLifecycle::Setup, value,),
            LifecycleDecision::Conflict
        );
    }

    #[test]
    fn final_departure_and_commitment_have_one_irreversible_winner() {
        let mut departure_first = CommitmentGate::new();
        departure_first.depart(CallerDeparture::Cancelled);
        assert_eq!(
            departure_first.commit(),
            CommitmentOutcome::Departed(CallerDeparture::Cancelled)
        );
        assert!(!departure_first.is_committed());

        let mut commitment_first = CommitmentGate::new();
        assert_eq!(commitment_first.commit(), CommitmentOutcome::Committed);
        commitment_first.depart(CallerDeparture::Disconnected);
        assert!(commitment_first.is_committed());
        assert_eq!(commitment_first.commit(), CommitmentOutcome::Committed);

        let mut duplicate = CommitmentGate::new();
        assert!(duplicate.join());
        duplicate.depart(CallerDeparture::Deadline);
        assert_eq!(duplicate.commit(), CommitmentOutcome::Committed);
    }

    #[test]
    fn no_change_and_submit_rows_follow_lifecycle_specific_contracts() {
        assert_eq!(
            decide_lifecycle(
                LifecycleAction::Up,
                WorkloadLifecycle::LongRunning,
                observation(LifecycleState::ActiveRunning),
            ),
            LifecycleDecision::NoChange
        );
        assert_eq!(
            decide_lifecycle(
                LifecycleAction::Up,
                WorkloadLifecycle::Job,
                observation(LifecycleState::ActiveRunning),
            ),
            LifecycleDecision::UnexpectedState
        );
        assert_eq!(
            decide_lifecycle(
                LifecycleAction::Down,
                WorkloadLifecycle::Setup,
                observation(LifecycleState::ActiveExited),
            ),
            LifecycleDecision::Submit(ManagerAction::Stop)
        );
        assert_eq!(
            decide_lifecycle(
                LifecycleAction::Restart,
                WorkloadLifecycle::Setup,
                observation(LifecycleState::ActiveExited),
            ),
            LifecycleDecision::Submit(ManagerAction::Restart)
        );
    }
}
