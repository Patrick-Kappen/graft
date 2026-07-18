//! Typed, allowlisted read-only workload observations.

use serde::{Deserialize, Deserializer, Serialize};

use crate::manifest::{
    LifecycleCapability, ObservabilityCapability, StartupIntent, WorkloadLifecycle,
    MAX_MANIFEST_STRING_BYTES,
};
use crate::protocol::{ConnectionIdentifier, ManifestGeneration, WorkerTarget};

/// Maximum workloads returned by one status page.
pub const MAX_STATUS_PAGE_SIZE: u16 = 256;

/// Bounded non-secret observation text validated at the wire boundary.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ObservationText(String);

impl ObservationText {
    /// Parses bounded non-empty text without control characters.
    ///
    /// # Errors
    ///
    /// Returns an error when the value is empty, exceeds manifest string bounds,
    /// or contains a control character.
    pub fn parse(value: impl Into<String>) -> Result<Self, &'static str> {
        let value = value.into();
        if value.is_empty()
            || value.len() > MAX_MANIFEST_STRING_BYTES
            || value.chars().any(char::is_control)
        {
            return Err("invalid bounded observation text");
        }
        Ok(Self(value))
    }

    /// Returns the validated text.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for ObservationText {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        Self::parse(value).map_err(serde::de::Error::custom)
    }
}

/// Manifest-bound workload selector accepted by read-only operations.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct WorkloadSelector {
    /// Fixed worker target.
    pub target: WorkerTarget,
    /// Validated workload name.
    pub name: ObservationText,
    /// Current manifest generation.
    pub generation: ManifestGeneration,
    /// Manifest-issued workload identity.
    pub workload_id: ManifestGeneration,
}

/// Opaque, worker-issued pagination token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PageCursor(pub(crate) String);

/// Optional status-list filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct StatusFilter {
    /// Include only this lifecycle when present.
    pub lifecycle: Option<WorkloadLifecycle>,
    /// Include only this summary when present.
    pub summary: Option<Summary>,
}

/// Request for one stable page of status summaries.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ListStatusRequest {
    /// Number of records from one through 256.
    pub page_size: u16,
    /// Cursor returned by the preceding page.
    pub cursor: Option<PageCursor>,
    /// Fixed typed filter.
    pub filter: Option<StatusFilter>,
}

/// Explicit availability of one observation layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "availability", content = "value", rename_all = "snake_case")]
pub enum Availability<T> {
    /// Current attributable evidence.
    Fresh(T),
    /// Retained evidence that cannot be treated as current.
    Stale(T),
    /// Applicable source could not be queried.
    Unavailable,
    /// Policy does not disclose this layer.
    Unauthorized,
    /// Worker or backend cannot provide this layer.
    Unsupported,
    /// Layer has no meaning for the current workload state.
    NotApplicable,
}

/// Materialisation evidence derived from the validated manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct MaterialisedState {
    /// Whether expected artifacts are present.
    pub artifacts_present: bool,
    /// Whether artifact validation succeeded.
    pub valid: bool,
    /// Materialised artifact identity.
    pub artifact_identity: ManifestGeneration,
    /// Closure identity without a host path.
    pub closure_identity: ManifestGeneration,
}

/// Generated-unit provenance evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct GeneratedState {
    /// Whether the expected generated service exists.
    pub service_present: bool,
    /// Whether a foreign source shadows the service.
    pub shadowed: bool,
    /// Whether source and producer provenance match.
    pub provenance_matches: bool,
}

/// Normalized manager active state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManagerActiveState {
    /// Unit is inactive.
    Inactive,
    /// Unit is starting.
    Activating,
    /// Long-running unit is active.
    ActiveRunning,
    /// Retained oneshot unit is active/exited.
    ActiveExited,
    /// Unit is stopping.
    Deactivating,
    /// Unit failed.
    Failed,
}

/// Allowlisted manager evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ManagerState {
    /// Whether the expected unit is loaded.
    pub unit_present: bool,
    /// Normalized active state when loaded.
    pub active_state: Option<ManagerActiveState>,
    /// Whether the current attributable invocation failed.
    pub terminal_failure: bool,
}

/// Normalized runtime state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeState {
    /// Container exists but is not running.
    Present,
    /// Container is running.
    Running,
    /// Expected container is absent.
    Absent,
}

/// Allowlisted runtime evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct RuntimeObservation {
    /// Normalized container state.
    pub state: RuntimeState,
    /// Whether runtime identity and manager cgroup attribution match.
    pub identity_matches: bool,
}

/// Deterministic summary derived from visible typed evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Summary {
    /// Disabled and no attributable residual work.
    Disabled,
    /// Disabled but attributable work remains active.
    DisabledRunning,
    /// Enabled expected artifacts are absent.
    NotMaterialised,
    /// Materialised or generated provenance is invalid.
    InvalidMaterialisation,
    /// Manager evidence is unavailable.
    ManagerUnavailable,
    /// Expected generated unit is absent.
    UnitMissing,
    /// A foreign unit shadows the expected service.
    UnitShadowed,
    /// Workload is inactive.
    Inactive,
    /// Workload is activating.
    Activating,
    /// Long-running workload is active.
    Active,
    /// Setup workload completed and remains active/exited.
    ActiveExited,
    /// Workload is deactivating.
    Deactivating,
    /// Current attributable invocation failed.
    Failed,
    /// Runtime identity or cgroup differs from manager attribution.
    RuntimeMismatch,
    /// Evidence is stale, unsupported, or insufficient.
    Unknown,
}

/// Safe manifest-derived declared layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct DeclaredLayer {
    /// Whether deployment intent is enabled.
    pub enabled: bool,
    /// Workload lifecycle.
    pub lifecycle: WorkloadLifecycle,
    /// Startup activation intent.
    pub startup_intent: StartupIntent,
    /// Non-secret source filename identity.
    pub source_identity: ObservationText,
    /// Source content digest.
    pub source_digest: ManifestGeneration,
    /// Allowed lifecycle operations.
    pub lifecycle_capabilities: Vec<LifecycleCapability>,
    /// Allowed observation layers.
    pub observability_capabilities: Vec<ObservabilityCapability>,
}

/// Safe manifest-derived resolved layer.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ResolvedLayer {
    /// Resolved configuration digest.
    pub resolved_digest: ManifestGeneration,
    /// Dependency graph digest.
    pub dependency_digest: ManifestGeneration,
    /// Resolver producer version.
    pub producer_version: ObservationText,
}

/// Cross-layer observation evidence.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ObservedLayer {
    /// Whether all attributable identities agree.
    pub identity_matches: bool,
}

/// One authorized layered workload snapshot.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct WorkloadSnapshot {
    /// Worker epoch that owns this observation.
    pub worker_epoch: ConnectionIdentifier,
    /// Effective user identity authorized for this observation.
    pub effective_uid: u32,
    /// Authorization policy revision used for this observation.
    pub authorization_revision: u64,
    /// Manifest-bound identity.
    pub selector: WorkloadSelector,
    /// Deterministic summary.
    pub summary: Summary,
    /// Declarative intent.
    pub declared: Availability<DeclaredLayer>,
    /// Resolver evidence.
    pub resolved: Availability<ResolvedLayer>,
    /// Materialisation evidence.
    pub materialised: Availability<MaterialisedState>,
    /// Generated-unit evidence.
    pub generated: Availability<GeneratedState>,
    /// Manager evidence.
    pub manager: Availability<ManagerState>,
    /// Runtime evidence.
    pub runtime: Availability<RuntimeObservation>,
    /// Worker-derived cross-layer evidence.
    pub observed: Availability<ObservedLayer>,
}

/// Bounded summary record returned by list operations.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct StatusSummary {
    /// Manifest-bound identity.
    pub selector: WorkloadSelector,
    /// Workload lifecycle.
    pub lifecycle: WorkloadLifecycle,
    /// Deterministic summary.
    pub summary: Summary,
}

/// One stable status page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct StatusPage {
    /// Worker epoch that owns this page and its cursors.
    pub worker_epoch: ConnectionIdentifier,
    /// Authorization policy revision that scopes this view.
    pub authorization_revision: u64,
    /// Authorization-scoped list revision.
    pub revision: u64,
    /// Ordered visible summaries.
    pub workloads: Vec<StatusSummary>,
    /// Cursor for the next page.
    pub next_cursor: Option<PageCursor>,
}

/// Full inspect result without raw backend payloads.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct InspectSnapshot {
    /// Layered status snapshot.
    pub snapshot: WorkloadSnapshot,
    /// Whether manager status is implemented by the selected adapter.
    pub manager_supported: bool,
    /// Whether runtime status is implemented by the selected adapter.
    pub runtime_supported: bool,
}

/// Inputs to the ordered summary table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SummaryEvidence {
    /// Deployment intent.
    pub enabled: bool,
    /// Lifecycle kind.
    pub lifecycle: WorkloadLifecycle,
    /// Materialisation layer.
    pub materialised: Availability<MaterialisedState>,
    /// Generated layer.
    pub generated: Availability<GeneratedState>,
    /// Manager layer.
    pub manager: Availability<ManagerState>,
    /// Runtime layer.
    pub runtime: Availability<RuntimeObservation>,
}

/// Applies the approved ordered first-match summary table.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn derive_summary(evidence: &SummaryEvidence) -> Summary {
    let manager_active = matches!(
        evidence.manager,
        Availability::Fresh(ManagerState {
            active_state: Some(
                ManagerActiveState::Activating
                    | ManagerActiveState::ActiveRunning
                    | ManagerActiveState::ActiveExited
                    | ManagerActiveState::Deactivating
            ),
            ..
        })
    );
    let runtime_active = matches!(
        evidence.runtime,
        Availability::Fresh(RuntimeObservation {
            state: RuntimeState::Present | RuntimeState::Running,
            ..
        })
    );
    if !evidence.enabled && (manager_active || runtime_active) {
        return Summary::DisabledRunning;
    }
    if !evidence.enabled {
        return Summary::Disabled;
    }
    if matches!(
        evidence.materialised,
        Availability::Fresh(MaterialisedState {
            artifacts_present: false,
            ..
        })
    ) {
        return Summary::NotMaterialised;
    }
    if matches!(
        evidence.generated,
        Availability::Fresh(GeneratedState { shadowed: true, .. })
    ) {
        return Summary::UnitShadowed;
    }
    if matches!(
        evidence.materialised,
        Availability::Fresh(MaterialisedState { valid: false, .. })
    ) || matches!(
        evidence.generated,
        Availability::Fresh(GeneratedState {
            provenance_matches: false,
            shadowed: false,
            ..
        })
    ) {
        return Summary::InvalidMaterialisation;
    }
    if matches!(evidence.manager, Availability::Unavailable) {
        return Summary::ManagerUnavailable;
    }
    if matches!(
        evidence.generated,
        Availability::Fresh(GeneratedState {
            service_present: false,
            ..
        })
    ) || matches!(
        evidence.manager,
        Availability::Fresh(ManagerState {
            unit_present: false,
            ..
        })
    ) {
        return Summary::UnitMissing;
    }
    if matches!(
        evidence.manager,
        Availability::Fresh(
            ManagerState {
                active_state: Some(ManagerActiveState::Failed),
                ..
            } | ManagerState {
                terminal_failure: true,
                ..
            }
        )
    ) {
        return Summary::Failed;
    }
    if matches!(
        evidence.manager,
        Availability::Fresh(ManagerState {
            active_state: Some(ManagerActiveState::Activating),
            ..
        })
    ) {
        return Summary::Activating;
    }
    if matches!(
        evidence.manager,
        Availability::Fresh(ManagerState {
            active_state: Some(ManagerActiveState::Deactivating),
            ..
        })
    ) {
        return Summary::Deactivating;
    }
    if matches!(
        evidence.runtime,
        Availability::Fresh(RuntimeObservation {
            identity_matches: false,
            ..
        })
    ) {
        return Summary::RuntimeMismatch;
    }
    if evidence.lifecycle == WorkloadLifecycle::Setup
        && matches!(
            evidence.manager,
            Availability::Fresh(ManagerState {
                active_state: Some(ManagerActiveState::ActiveExited),
                terminal_failure: false,
                ..
            })
        )
    {
        return Summary::ActiveExited;
    }
    if evidence.lifecycle == WorkloadLifecycle::LongRunning
        && matches!(
            evidence.manager,
            Availability::Fresh(ManagerState {
                active_state: Some(ManagerActiveState::ActiveRunning),
                terminal_failure: false,
                ..
            })
        )
    {
        return Summary::Active;
    }
    if matches!(
        evidence.manager,
        Availability::Fresh(ManagerState {
            active_state: Some(ManagerActiveState::Inactive),
            terminal_failure: false,
            ..
        })
    ) && !runtime_active
    {
        return Summary::Inactive;
    }
    Summary::Unknown
}

#[cfg(test)]
mod tests {
    use super::*;

    fn identity() -> ManifestGeneration {
        ManifestGeneration::parse("a".repeat(64)).unwrap()
    }

    fn evidence() -> SummaryEvidence {
        SummaryEvidence {
            enabled: true,
            lifecycle: WorkloadLifecycle::LongRunning,
            materialised: Availability::Fresh(MaterialisedState {
                artifacts_present: true,
                valid: true,
                artifact_identity: identity(),
                closure_identity: identity(),
            }),
            generated: Availability::Fresh(GeneratedState {
                service_present: true,
                shadowed: false,
                provenance_matches: true,
            }),
            manager: Availability::Fresh(ManagerState {
                unit_present: true,
                active_state: Some(ManagerActiveState::Inactive),
                terminal_failure: false,
            }),
            runtime: Availability::Fresh(RuntimeObservation {
                state: RuntimeState::Absent,
                identity_matches: true,
            }),
        }
    }

    #[test]
    fn observation_text_enforces_manifest_string_bounds_at_deserialization() {
        assert!(ObservationText::parse("a".repeat(MAX_MANIFEST_STRING_BYTES)).is_ok());
        assert!(ObservationText::parse("a".repeat(MAX_MANIFEST_STRING_BYTES + 1)).is_err());
        assert!(ObservationText::parse("").is_err());
        assert!(ObservationText::parse("unsafe\nvalue").is_err());
        assert!(serde_json::from_str::<ObservationText>("\"unsafe\\nvalue\"").is_err());
    }

    #[test]
    fn summary_table_covers_disabled_setup_job_stale_missing_and_unavailable_states() {
        let mut value = evidence();
        value.enabled = false;
        assert_eq!(derive_summary(&value), Summary::Disabled);

        value.manager = Availability::Fresh(ManagerState {
            unit_present: true,
            active_state: Some(ManagerActiveState::ActiveRunning),
            terminal_failure: false,
        });
        assert_eq!(derive_summary(&value), Summary::DisabledRunning);

        value = evidence();
        value.lifecycle = WorkloadLifecycle::Setup;
        value.manager = Availability::Fresh(ManagerState {
            unit_present: true,
            active_state: Some(ManagerActiveState::ActiveExited),
            terminal_failure: false,
        });
        assert_eq!(derive_summary(&value), Summary::ActiveExited);

        value = evidence();
        value.lifecycle = WorkloadLifecycle::Job;
        assert_eq!(derive_summary(&value), Summary::Inactive);
        value.manager = Availability::Fresh(ManagerState {
            unit_present: true,
            active_state: Some(ManagerActiveState::Inactive),
            terminal_failure: true,
        });
        assert_eq!(derive_summary(&value), Summary::Failed);

        value = evidence();
        value.manager = Availability::Stale(ManagerState {
            unit_present: true,
            active_state: Some(ManagerActiveState::ActiveRunning),
            terminal_failure: false,
        });
        assert_eq!(derive_summary(&value), Summary::Unknown);

        value = evidence();
        value.manager = Availability::Fresh(ManagerState {
            unit_present: false,
            active_state: None,
            terminal_failure: false,
        });
        assert_eq!(derive_summary(&value), Summary::UnitMissing);

        value = evidence();
        value.manager = Availability::Unavailable;
        assert_eq!(derive_summary(&value), Summary::ManagerUnavailable);

        value = evidence();
        value.manager = Availability::Fresh(ManagerState {
            unit_present: true,
            active_state: Some(ManagerActiveState::Activating),
            terminal_failure: false,
        });
        assert_eq!(derive_summary(&value), Summary::Activating);

        value.manager = Availability::Fresh(ManagerState {
            unit_present: true,
            active_state: Some(ManagerActiveState::Deactivating),
            terminal_failure: false,
        });
        assert_eq!(derive_summary(&value), Summary::Deactivating);

        value.manager = Availability::Fresh(ManagerState {
            unit_present: true,
            active_state: Some(ManagerActiveState::ActiveRunning),
            terminal_failure: false,
        });
        assert_eq!(derive_summary(&value), Summary::Active);

        value.runtime = Availability::Fresh(RuntimeObservation {
            state: RuntimeState::Running,
            identity_matches: false,
        });
        assert_eq!(derive_summary(&value), Summary::RuntimeMismatch);

        value = evidence();
        value.materialised = Availability::Fresh(MaterialisedState {
            artifacts_present: true,
            valid: false,
            artifact_identity: identity(),
            closure_identity: identity(),
        });
        assert_eq!(derive_summary(&value), Summary::InvalidMaterialisation);
    }

    #[test]
    fn summary_table_preserves_first_match_priority() {
        let mut value = evidence();
        value.enabled = false;
        value.generated = Availability::Fresh(GeneratedState {
            service_present: true,
            shadowed: true,
            provenance_matches: false,
        });
        assert_eq!(derive_summary(&value), Summary::Disabled);

        value = evidence();
        value.materialised = Availability::Fresh(MaterialisedState {
            artifacts_present: false,
            valid: false,
            artifact_identity: identity(),
            closure_identity: identity(),
        });
        value.generated = Availability::Fresh(GeneratedState {
            service_present: true,
            shadowed: true,
            provenance_matches: false,
        });
        assert_eq!(derive_summary(&value), Summary::NotMaterialised);

        value = evidence();
        value.generated = Availability::Fresh(GeneratedState {
            service_present: true,
            shadowed: true,
            provenance_matches: false,
        });
        value.manager = Availability::Unavailable;
        assert_eq!(derive_summary(&value), Summary::UnitShadowed);

        value = evidence();
        value.manager = Availability::Fresh(ManagerState {
            unit_present: true,
            active_state: Some(ManagerActiveState::Failed),
            terminal_failure: true,
        });
        value.runtime = Availability::Fresh(RuntimeObservation {
            state: RuntimeState::Running,
            identity_matches: false,
        });
        assert_eq!(derive_summary(&value), Summary::Failed);
    }
}
