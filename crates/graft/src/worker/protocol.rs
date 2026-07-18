//! Typed post-handshake worker transport envelopes.

use serde::{Deserialize, Serialize};

use crate::protocol::{ConnectionIdentifier, RequestIdentifier, SafeSummary};

use super::observation::{
    InspectSnapshot, ListStatusRequest, StatusPage, WorkloadSelector, WorkloadSnapshot,
};

/// Client frame accepted after a successful handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ClientFrame {
    /// Starts one typed semantic operation.
    Request(Request),
    /// Acknowledges the highest contiguous stream sequence consumed.
    StreamAck(StreamAck),
    /// Cancels this connection's interest in one request.
    Cancel(Cancel),
}

/// Server frame emitted after a successful handshake.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum ServerFrame {
    /// Completes one unary request.
    Response(Response),
    /// Reports a non-terminal rejected duplicate request.
    RequestError(RequestError),
    /// Reports a non-terminal control-frame failure.
    ControlError(ControlError),
    /// Emits one sequenced stream item.
    StreamItem(StreamItem),
    /// Ends one stream.
    StreamEnd(StreamEnd),
}

/// Typed request envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct Request {
    /// Server-selected connection identifier from `ServerHello`.
    pub server_connection_id: ConnectionIdentifier,
    /// Non-zero connection-local request identifier.
    pub request_id: RequestIdentifier,
    /// Optional duration from complete-frame receipt.
    pub deadline_ms: Option<u64>,
    /// Typed semantic operation.
    pub operation: SemanticRequest,
}

/// Stream acknowledgement targeting one active request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct StreamAck {
    /// Server-selected connection identifier from `ServerHello`.
    pub server_connection_id: ConnectionIdentifier,
    /// Active request identifier.
    pub request_id: RequestIdentifier,
    /// Highest contiguous consumed sequence.
    pub sequence: u64,
}

/// Cancellation targeting one active request.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct Cancel {
    /// Server-selected connection identifier from `ServerHello`.
    pub server_connection_id: ConnectionIdentifier,
    /// Active request identifier.
    pub request_id: RequestIdentifier,
}

/// Semantic requests are supplied by later worker slices.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum SemanticRequest {
    /// Lists one bounded stable page of user-scope status summaries.
    ListStatus(ListStatusRequest),
    /// Gets one manifest-bound layered status snapshot.
    GetStatus {
        /// Exact current workload identity.
        selector: WorkloadSelector,
    },
    /// Gets one full allowlisted inspect snapshot.
    Inspect {
        /// Exact current workload identity.
        selector: WorkloadSelector,
    },
    /// Internal production placeholder that is never accepted from the wire.
    #[serde(skip)]
    Reserved,
    /// Deterministic fixture unary response.
    #[cfg(feature = "worker-test-fixtures")]
    MockUnary {
        /// Milliseconds to wait before responding.
        delay_ms: u64,
    },
    /// Deterministic fixture lifecycle-class response.
    #[cfg(feature = "worker-test-fixtures")]
    MockLifecycle {
        /// Milliseconds to wait before responding.
        delay_ms: u64,
    },
    /// Deterministic fixture stream.
    #[cfg(feature = "worker-test-fixtures")]
    MockStream {
        /// Number of items to emit.
        items: u32,
        /// Milliseconds between items.
        interval_ms: u64,
    },
}

/// Non-terminal error rejecting a new request while its identifier is active.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct RequestError {
    /// Server-selected connection identifier.
    pub server_connection_id: ConnectionIdentifier,
    /// Active request identifier that rejected the new request.
    pub request_id: RequestIdentifier,
    /// Typed admission failure.
    pub error: WorkerError,
}

/// Non-terminal error for a stream control frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct ControlError {
    /// Server-selected connection identifier.
    pub server_connection_id: ConnectionIdentifier,
    /// Request identifier targeted by the invalid control frame.
    pub request_id: RequestIdentifier,
    /// Typed control failure.
    pub error: WorkerError,
}

/// Unary response envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct Response {
    /// Server-selected connection identifier.
    pub server_connection_id: ConnectionIdentifier,
    /// Request identifier being completed.
    pub request_id: RequestIdentifier,
    /// Typed response result.
    pub result: ResponseResult,
}

/// Typed unary result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case", deny_unknown_fields)]
pub enum ResponseResult {
    /// Read-only discovery or status operation completed.
    ReadOnly(Box<ReadOnlyResponse>),
    /// Fixture operation completed.
    #[cfg(feature = "worker-test-fixtures")]
    MockComplete,
    /// Request failed safely.
    Error(WorkerError),
}

/// Successful read-only operation result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "response", rename_all = "snake_case", deny_unknown_fields)]
pub enum ReadOnlyResponse {
    /// One stable page of summaries.
    StatusPage(StatusPage),
    /// One layered status snapshot.
    Status(WorkloadSnapshot),
    /// One full allowlisted inspect snapshot.
    Inspect(InspectSnapshot),
}

/// Sequenced fixture stream item.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct StreamItem {
    /// Server-selected connection identifier.
    pub server_connection_id: ConnectionIdentifier,
    /// Request identifier owning the stream.
    pub request_id: RequestIdentifier,
    /// Worker epoch that owns this sequence.
    pub worker_epoch: ConnectionIdentifier,
    /// Monotone sequence starting at one.
    pub sequence: u64,
    /// Fixture payload value.
    #[cfg(feature = "worker-test-fixtures")]
    pub mock_value: u32,
}

/// Terminal stream envelope.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct StreamEnd {
    /// Server-selected connection identifier.
    pub server_connection_id: ConnectionIdentifier,
    /// Request identifier owning the stream.
    pub request_id: RequestIdentifier,
    /// Worker epoch that owned this sequence.
    pub worker_epoch: ConnectionIdentifier,
    /// Terminal reason.
    pub reason: StreamEndReason,
    /// Last sequence emitted, or zero when no item was emitted.
    pub final_sequence: u64,
}

/// Closed stream-end reasons.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StreamEndReason {
    /// Producer completed normally.
    Completed,
    /// Caller cancelled interest.
    Cancelled,
    /// Caller deadline elapsed.
    Deadline,
    /// Client did not acknowledge within the bounded window.
    SlowConsumer,
    /// Worker is shutting down.
    WorkerShutdown,
}

/// Stable post-handshake error.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "snake_case")]
pub struct WorkerError {
    /// Stable closed error code.
    pub code: WorkerErrorCode,
    /// Safe bounded explanation.
    pub summary: SafeSummary,
    /// Stable client retry guidance.
    pub retry: RetryClassification,
    /// Operation phase in which the error occurred.
    pub phase: OperationPhase,
    /// Worker epoch that produced the error.
    pub worker_epoch: ConnectionIdentifier,
}

/// Closed client retry classifications.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RetryClassification {
    /// Repeating the same request cannot make it valid.
    Never,
    /// Refresh state before deciding whether a new request is safe.
    AfterStateRefresh,
    /// Re-authorize before deciding whether a new request is safe.
    AfterAuthorization,
    /// Wait for backend recovery before issuing a new request.
    AfterBackendRecovery,
    /// The same idempotent request may be retried with bounded backoff.
    SameRequestWithBackoff,
}

/// Closed operation phases represented by the server core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OperationPhase {
    /// The request is being admitted and validated.
    Admission,
    /// The typed semantic operation is being dispatched.
    Dispatch,
    /// The admitted operation is executing.
    Execution,
    /// A stream control frame or stream producer failed.
    Stream,
}

/// Closed post-handshake error codes used by the server core.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkerErrorCode {
    /// A fixed or negotiated budget is exhausted.
    Overloaded,
    /// Request identifier is already active.
    RequestConflict,
    /// Connection identifier does not match this connection.
    ConnectionMismatch,
    /// Request deadline is invalid or elapsed.
    Deadline,
    /// Caller cancelled the request.
    Cancelled,
    /// Worker is shutting down.
    WorkerShutdown,
    /// Request does not identify an active operation.
    RequestNotFound,
    /// Stream acknowledgement is invalid.
    InvalidAcknowledgement,
    /// Semantic operation is unavailable in this build.
    Unsupported,
    /// Peer or fixed context is not authorized, without existence disclosure.
    Unauthorized,
    /// Current manifest cannot be loaded coherently.
    ManifestUnavailable,
    /// Request targets an old manifest generation.
    StaleManifest,
    /// Workload is not visible under the current authorization.
    WorkloadNotFound,
    /// Pagination cursor no longer matches current visible state.
    PageCursorExpired,
    /// Typed request values violate semantic bounds.
    InvalidRequest,
    /// Internal read-only state is temporarily unavailable.
    Unavailable,
}
