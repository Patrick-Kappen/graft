//! Typed connection handshake and deterministic version/limit negotiation.

use std::cmp;

use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::types::{
    Capability, CapabilitySet, ClientComponent, ConnectionIdentifier, ManagerKind,
    ManifestGeneration, ProtocolVersion, ProtocolVersionRange, SafeSummary, ServerTimeMilliseconds,
    SoftwareVersion, ValidationError, WorkerTarget,
};

/// Maximum values a client may request in protocol version 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolMaxima;

impl ProtocolMaxima {
    /// Maximum concurrent requests on one connection.
    pub const CONCURRENT_REQUESTS: u32 = 32;
    /// Maximum active streams on one connection.
    pub const ACTIVE_STREAMS: u32 = 8;
    /// Maximum buffered response bytes for one principal.
    pub const BUFFERED_RESPONSE_BYTES: u32 = 2 * 1024 * 1024;
    /// Maximum unacknowledged items in one stream.
    pub const UNACKNOWLEDGED_STREAM_ITEMS: u32 = 64;
    /// Maximum workloads in one list page.
    pub const WORKLOADS_PER_PAGE: u32 = 256;
    /// Maximum historical log records in one page.
    pub const LOG_RECORDS_PER_PAGE: u32 = 1_000;
    /// Maximum encoded log-message bytes in one item.
    pub const ENCODED_LOG_MESSAGE_BYTES: u32 = 64 * 1024;
    /// Maximum unary request deadline in milliseconds.
    pub const UNARY_DEADLINE_MS: u64 = 60_000;
    /// Maximum lifecycle request deadline in milliseconds.
    pub const LIFECYCLE_DEADLINE_MS: u64 = 300_000;
}

/// Requested or negotiated operation limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct EffectiveLimits {
    concurrent_requests: u32,
    active_streams: u32,
    buffered_response_bytes: u32,
    unacknowledged_stream_items: u32,
    workloads_per_page: u32,
    log_records_per_page: u32,
    encoded_log_message_bytes: u32,
    unary_deadline_ms: u64,
    lifecycle_deadline_ms: u64,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct EffectiveLimitsWire {
    concurrent_requests: u32,
    active_streams: u32,
    buffered_response_bytes: u32,
    unacknowledged_stream_items: u32,
    workloads_per_page: u32,
    log_records_per_page: u32,
    encoded_log_message_bytes: u32,
    unary_deadline_ms: u64,
    lifecycle_deadline_ms: u64,
}

impl EffectiveLimits {
    /// Creates limits after checking every version-1 protocol maximum.
    ///
    /// # Errors
    ///
    /// Returns an error when a value is zero or exceeds its protocol maximum.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        concurrent_requests: u32,
        active_streams: u32,
        buffered_response_bytes: u32,
        unacknowledged_stream_items: u32,
        workloads_per_page: u32,
        log_records_per_page: u32,
        encoded_log_message_bytes: u32,
        unary_deadline_ms: u64,
        lifecycle_deadline_ms: u64,
    ) -> Result<Self, ValidationError> {
        validate_limit(
            "concurrent_requests",
            u64::from(concurrent_requests),
            u64::from(ProtocolMaxima::CONCURRENT_REQUESTS),
        )?;
        validate_limit(
            "active_streams",
            u64::from(active_streams),
            u64::from(ProtocolMaxima::ACTIVE_STREAMS),
        )?;
        validate_limit(
            "buffered_response_bytes",
            u64::from(buffered_response_bytes),
            u64::from(ProtocolMaxima::BUFFERED_RESPONSE_BYTES),
        )?;
        validate_limit(
            "unacknowledged_stream_items",
            u64::from(unacknowledged_stream_items),
            u64::from(ProtocolMaxima::UNACKNOWLEDGED_STREAM_ITEMS),
        )?;
        validate_limit(
            "workloads_per_page",
            u64::from(workloads_per_page),
            u64::from(ProtocolMaxima::WORKLOADS_PER_PAGE),
        )?;
        validate_limit(
            "log_records_per_page",
            u64::from(log_records_per_page),
            u64::from(ProtocolMaxima::LOG_RECORDS_PER_PAGE),
        )?;
        validate_limit(
            "encoded_log_message_bytes",
            u64::from(encoded_log_message_bytes),
            u64::from(ProtocolMaxima::ENCODED_LOG_MESSAGE_BYTES),
        )?;
        validate_limit(
            "unary_deadline_ms",
            unary_deadline_ms,
            ProtocolMaxima::UNARY_DEADLINE_MS,
        )?;
        validate_limit(
            "lifecycle_deadline_ms",
            lifecycle_deadline_ms,
            ProtocolMaxima::LIFECYCLE_DEADLINE_MS,
        )?;
        Ok(Self {
            concurrent_requests,
            active_streams,
            buffered_response_bytes,
            unacknowledged_stream_items,
            workloads_per_page,
            log_records_per_page,
            encoded_log_message_bytes,
            unary_deadline_ms,
            lifecycle_deadline_ms,
        })
    }

    /// Returns all version-1 protocol maxima.
    #[must_use]
    pub fn protocol_maxima() -> Self {
        Self {
            concurrent_requests: ProtocolMaxima::CONCURRENT_REQUESTS,
            active_streams: ProtocolMaxima::ACTIVE_STREAMS,
            buffered_response_bytes: ProtocolMaxima::BUFFERED_RESPONSE_BYTES,
            unacknowledged_stream_items: ProtocolMaxima::UNACKNOWLEDGED_STREAM_ITEMS,
            workloads_per_page: ProtocolMaxima::WORKLOADS_PER_PAGE,
            log_records_per_page: ProtocolMaxima::LOG_RECORDS_PER_PAGE,
            encoded_log_message_bytes: ProtocolMaxima::ENCODED_LOG_MESSAGE_BYTES,
            unary_deadline_ms: ProtocolMaxima::UNARY_DEADLINE_MS,
            lifecycle_deadline_ms: ProtocolMaxima::LIFECYCLE_DEADLINE_MS,
        }
    }

    /// Returns limits no greater than either input.
    #[must_use]
    pub fn intersect(self, other: Self) -> Self {
        Self {
            concurrent_requests: cmp::min(self.concurrent_requests, other.concurrent_requests),
            active_streams: cmp::min(self.active_streams, other.active_streams),
            buffered_response_bytes: cmp::min(
                self.buffered_response_bytes,
                other.buffered_response_bytes,
            ),
            unacknowledged_stream_items: cmp::min(
                self.unacknowledged_stream_items,
                other.unacknowledged_stream_items,
            ),
            workloads_per_page: cmp::min(self.workloads_per_page, other.workloads_per_page),
            log_records_per_page: cmp::min(self.log_records_per_page, other.log_records_per_page),
            encoded_log_message_bytes: cmp::min(
                self.encoded_log_message_bytes,
                other.encoded_log_message_bytes,
            ),
            unary_deadline_ms: cmp::min(self.unary_deadline_ms, other.unary_deadline_ms),
            lifecycle_deadline_ms: cmp::min(
                self.lifecycle_deadline_ms,
                other.lifecycle_deadline_ms,
            ),
        }
    }

    /// Returns the maximum concurrent requests on one connection.
    #[must_use]
    pub const fn concurrent_requests(self) -> u32 {
        self.concurrent_requests
    }

    /// Returns the maximum active streams on one connection.
    #[must_use]
    pub const fn active_streams(self) -> u32 {
        self.active_streams
    }

    /// Returns the unary deadline bound in milliseconds.
    #[must_use]
    pub const fn unary_deadline_ms(self) -> u64 {
        self.unary_deadline_ms
    }

    /// Returns whether every limit is no greater than the corresponding bound.
    #[must_use]
    pub const fn is_within(self, bound: Self) -> bool {
        self.concurrent_requests <= bound.concurrent_requests
            && self.active_streams <= bound.active_streams
            && self.buffered_response_bytes <= bound.buffered_response_bytes
            && self.unacknowledged_stream_items <= bound.unacknowledged_stream_items
            && self.workloads_per_page <= bound.workloads_per_page
            && self.log_records_per_page <= bound.log_records_per_page
            && self.encoded_log_message_bytes <= bound.encoded_log_message_bytes
            && self.unary_deadline_ms <= bound.unary_deadline_ms
            && self.lifecycle_deadline_ms <= bound.lifecycle_deadline_ms
    }
}

impl<'de> Deserialize<'de> for EffectiveLimits {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = EffectiveLimitsWire::deserialize(deserializer)?;
        Self::new(
            wire.concurrent_requests,
            wire.active_streams,
            wire.buffered_response_bytes,
            wire.unacknowledged_stream_items,
            wire.workloads_per_page,
            wire.log_records_per_page,
            wire.encoded_log_message_bytes,
            wire.unary_deadline_ms,
            wire.lifecycle_deadline_ms,
        )
        .map_err(serde::de::Error::custom)
    }
}

fn validate_limit(field: &'static str, value: u64, maximum: u64) -> Result<(), ValidationError> {
    if value == 0 || value > maximum {
        return Err(ValidationError::InvalidLimit { field, maximum });
    }
    Ok(())
}

/// First typed frame sent by a client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ClientHello {
    /// Contiguous protocol range supported by the client.
    pub protocol: ProtocolVersionRange,
    /// Client component kind.
    pub component: ClientComponent,
    /// Diagnostic client software version.
    pub software_version: SoftwareVersion,
    /// Capabilities the client requires for this connection.
    pub requested_capabilities: CapabilitySet,
    /// Requested effective limits.
    pub requested_limits: EffectiveLimits,
    /// Client-generated diagnostic connection identifier.
    pub client_connection_id: ConnectionIdentifier,
}

/// Fixed worker context returned by the server.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct WorkerContext {
    target: WorkerTarget,
    effective_uid: u32,
    manager: ManagerKind,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
struct WorkerContextWire {
    target: WorkerTarget,
    effective_uid: u32,
    manager: ManagerKind,
}

impl WorkerContext {
    /// Creates one fixed approved worker context.
    ///
    /// # Errors
    ///
    /// Returns an error unless system target uses the system manager and UID 0,
    /// or user target uses the user manager.
    pub const fn new(
        target: WorkerTarget,
        effective_uid: u32,
        manager: ManagerKind,
    ) -> Result<Self, ValidationError> {
        let valid = match (target, manager) {
            (WorkerTarget::System, ManagerKind::System) => effective_uid == 0,
            (WorkerTarget::User, ManagerKind::User) => true,
            (WorkerTarget::System, ManagerKind::User)
            | (WorkerTarget::User, ManagerKind::System) => false,
        };
        if !valid {
            return Err(ValidationError::InvalidWorkerContext);
        }
        Ok(Self {
            target,
            effective_uid,
            manager,
        })
    }

    /// Returns the fixed target.
    #[must_use]
    pub const fn target(self) -> WorkerTarget {
        self.target
    }

    /// Returns the runtime effective UID.
    #[must_use]
    pub const fn effective_uid(self) -> u32 {
        self.effective_uid
    }

    /// Returns the fixed manager kind.
    #[must_use]
    pub const fn manager(self) -> ManagerKind {
        self.manager
    }
}

impl<'de> Deserialize<'de> for WorkerContext {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let wire = WorkerContextWire::deserialize(deserializer)?;
        Self::new(wire.target, wire.effective_uid, wire.manager).map_err(serde::de::Error::custom)
    }
}

/// Manifest state visible during the handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case", deny_unknown_fields)]
pub enum ManifestState {
    /// A validated current generation is available.
    Available {
        /// Current lowercase SHA-256 generation.
        generation: ManifestGeneration,
    },
    /// No validated current generation is available.
    Unavailable {
        /// Safe typed reason.
        reason: ManifestUnavailableReason,
    },
}

/// Safe manifest-unavailability reason exposed during negotiation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ManifestUnavailableReason {
    /// Current reference is absent.
    Missing,
    /// Current generation is malformed or fails validation.
    Invalid,
    /// Producer/schema compatibility does not overlap.
    Incompatible,
    /// Current generation cannot be read safely.
    Unreadable,
}

/// Successful server handshake response.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ServerHello {
    /// Selected protocol version.
    pub protocol: ProtocolVersion,
    /// Diagnostic worker software version.
    pub software_version: SoftwareVersion,
    /// Fixed worker context.
    pub context: WorkerContext,
    /// Capabilities supported and requested on this connection.
    pub capabilities: CapabilitySet,
    /// Negotiated effective limits.
    pub effective_limits: EffectiveLimits,
    /// Current manifest state.
    pub manifest: ManifestState,
    /// Current worker epoch.
    pub worker_epoch: ConnectionIdentifier,
    /// Logical server receive time in Unix milliseconds.
    pub server_time_ms: ServerTimeMilliseconds,
    /// Server-generated connection identifier used after the handshake.
    pub server_connection_id: ConnectionIdentifier,
}

/// Stable pre-handshake protocol error code.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProtocolErrorCode {
    /// Frame or JSON syntax is malformed.
    Malformed,
    /// Declared or encoded limit is exceeded.
    LimitExceeded,
    /// Protocol major/minor ranges do not overlap.
    UnsupportedVersion,
    /// A requested capability is unsupported.
    UnsupportedCapability,
}

/// Safe typed pre-handshake error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ProtocolError {
    /// Stable error code.
    pub code: ProtocolErrorCode,
    /// Safe bounded summary without raw client/backend input.
    pub summary: SafeSummary,
}

/// Client-to-server handshake frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientHandshakeFrame {
    /// Initial client negotiation frame.
    ClientHello(ClientHello),
}

/// Server-to-client handshake frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServerHandshakeFrame {
    /// Successful negotiation.
    ServerHello(ServerHello),
    /// Safe protocol failure followed by connection close.
    ProtocolError(ProtocolError),
}

/// Server-owned values used to negotiate a client hello.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerHandshakeConfig {
    /// Contiguous protocol range supported by the worker.
    pub protocol: ProtocolVersionRange,
    /// Worker software version.
    pub software_version: SoftwareVersion,
    /// Fixed worker context.
    pub context: WorkerContext,
    /// Worker capabilities.
    pub capabilities: CapabilitySet,
    /// Nix-configured effective maxima.
    pub limits: EffectiveLimits,
    /// Current manifest state.
    pub manifest: ManifestState,
    /// Current worker epoch.
    pub worker_epoch: ConnectionIdentifier,
    /// Logical receive time.
    pub server_time_ms: ServerTimeMilliseconds,
    /// Server-generated connection identifier.
    pub server_connection_id: ConnectionIdentifier,
}

/// Error returned by deterministic handshake negotiation.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum HandshakeError {
    /// Protocol ranges do not overlap.
    #[error("protocol version ranges do not overlap")]
    UnsupportedVersion,
    /// The client requires a capability not supported by the worker.
    #[error("requested capability is unsupported: {0}")]
    UnsupportedCapability(Capability),
    /// The server selected a version outside the client range.
    #[error("server selected a protocol version outside the client range")]
    InvalidServerVersion,
    /// The server omitted a capability required by the client.
    #[error("server omitted a requested capability: {0}")]
    MissingServerCapability(Capability),
    /// The server returned a limit above the client request.
    #[error("server returned an effective limit above the client request")]
    InvalidServerLimits,
}

/// Negotiates a validated client hello against fixed server policy.
///
/// # Errors
///
/// Returns an error when versions do not overlap, a requested capability is
/// unavailable.
pub fn negotiate_handshake(
    client: &ClientHello,
    server: &ServerHandshakeConfig,
) -> Result<ServerHello, HandshakeError> {
    if client.protocol.major() != server.protocol.major() {
        return Err(HandshakeError::UnsupportedVersion);
    }
    let minimum = cmp::max(client.protocol.min_minor(), server.protocol.min_minor());
    let maximum = cmp::min(client.protocol.max_minor(), server.protocol.max_minor());
    if minimum > maximum {
        return Err(HandshakeError::UnsupportedVersion);
    }
    for capability in client.requested_capabilities.iter() {
        if !server.capabilities.contains(capability) {
            return Err(HandshakeError::UnsupportedCapability(capability));
        }
    }
    Ok(ServerHello {
        protocol: ProtocolVersion::new(client.protocol.major(), maximum),
        software_version: server.software_version.clone(),
        context: server.context,
        capabilities: server.capabilities.clone(),
        effective_limits: client.requested_limits.intersect(server.limits),
        manifest: server.manifest.clone(),
        worker_epoch: server.worker_epoch,
        server_time_ms: server.server_time_ms,
        server_connection_id: server.server_connection_id,
    })
}

/// Validates a server hello against the client hello that preceded it.
///
/// # Errors
///
/// Returns an error when the selected version is outside the client range, a
/// requested capability is absent, or an effective limit exceeds the request.
pub fn validate_server_hello(
    client: &ClientHello,
    server: &ServerHello,
) -> Result<(), HandshakeError> {
    if server.protocol.major() != client.protocol.major()
        || server.protocol.minor() < client.protocol.min_minor()
        || server.protocol.minor() > client.protocol.max_minor()
    {
        return Err(HandshakeError::InvalidServerVersion);
    }
    for capability in client.requested_capabilities.iter() {
        if !server.capabilities.contains(capability) {
            return Err(HandshakeError::MissingServerCapability(capability));
        }
    }
    if !server.effective_limits.is_within(client.requested_limits) {
        return Err(HandshakeError::InvalidServerLimits);
    }
    Ok(())
}
