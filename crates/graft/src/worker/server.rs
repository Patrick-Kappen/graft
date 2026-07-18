//! Bounded socket-activated Unix worker server.

use std::collections::BTreeMap;
use std::io;
use std::os::fd::AsFd as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncWriteExt as _, WriteHalf};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot, watch, Notify, Semaphore};

use crate::protocol::{
    encode_frame_exact, encoded_frame_len, negotiate_handshake, CapabilitySet,
    ClientHandshakeFrame, ConnectionIdentifier, EffectiveLimits, FrameDirection, HandshakeError,
    ManifestState, ProtocolError, ProtocolErrorCode, ProtocolVersionRange, SafeSummary,
    ServerHandshakeConfig, ServerHandshakeFrame, SoftwareVersion, WorkerContext,
};

use super::clock::WorkerEpoch;
use super::dispatcher::{
    DispatchContext, DispatchPlan, PeerCredentials, PrincipalKey, SemanticDispatcher,
};
#[cfg(feature = "worker-test-fixtures")]
use super::framing::PARTIAL_FRAME_TIMEOUT;
use super::framing::{read_frame, read_frame_with_timestamp, write_frame, AsyncFrameError};
use super::limits::{AdmissionPermit, AdmissionRegistry, ConnectionBuffer, ConnectionBufferPermit};
use super::protocol::{
    ClientFrame, ControlError, OperationPhase, Request, Response, ResponseResult,
    RetryClassification, SemanticRequest, ServerFrame, WorkerError, WorkerErrorCode,
};
#[cfg(feature = "worker-test-fixtures")]
use super::protocol::{StreamEnd, StreamEndReason, StreamItem};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const OUTBOUND_QUEUE_ITEMS: usize = 64;
const TERMINAL_DELIVERY_TIMEOUT: Duration = Duration::from_secs(5);

/// Fixed server configuration supplied by installed policy.
#[derive(Debug, Clone)]
pub struct ServerConfig {
    /// Fixed worker execution context.
    pub context: WorkerContext,
    /// Worker protocol range.
    pub protocol: ProtocolVersionRange,
    /// Diagnostic worker package version.
    pub software_version: SoftwareVersion,
    /// Supported semantic capabilities.
    pub capabilities: CapabilitySet,
    /// Nix-lowered worker maxima.
    pub limits: EffectiveLimits,
    /// Validated current manifest state.
    pub manifest: ManifestState,
    /// Typed semantic dispatcher.
    pub dispatcher: Arc<dyn SemanticDispatcher>,
}

/// Server startup or runtime failure.
#[derive(Debug, thiserror::Error)]
pub enum ServerError {
    /// Worker epoch could not be created.
    #[error("failed to create worker epoch")]
    Epoch,
    /// Activated listener setup failed.
    #[error("failed to configure activated listener")]
    Listener(#[source] io::Error),
    /// Process shutdown signal registration failed.
    #[error("failed to register process shutdown signals")]
    Signal(#[source] io::Error),
    /// Activated listener encountered a persistent accept failure.
    #[error("activated listener accept failed")]
    Accept(#[source] io::Error),
}

fn peer_is_authorized(context: WorkerContext, peer_uid: u32) -> bool {
    context.target() != crate::protocol::WorkerTarget::User || peer_uid == context.effective_uid()
}

/// Runs one worker process until shutdown.
///
/// # Errors
///
/// Returns an error when epoch or listener setup fails. Individual hostile or
/// disconnected clients are isolated to their connection.
pub async fn serve(
    listener: std::os::unix::net::UnixListener,
    config: ServerConfig,
) -> Result<(), ServerError> {
    listener
        .set_nonblocking(true)
        .map_err(ServerError::Listener)?;
    let listener = UnixListener::from_std(listener).map_err(ServerError::Listener)?;
    let epoch = Arc::new(WorkerEpoch::new().map_err(|_| ServerError::Epoch)?);
    let registry = AdmissionRegistry::with_buffered_bytes_per_principal(
        usize::try_from(config.limits.buffered_response_bytes()).unwrap_or(usize::MAX),
    );
    let mut terminate = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        .map_err(ServerError::Signal)?;
    let mut interrupt = tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        .map_err(ServerError::Signal)?;
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    tokio::spawn(async move {
        tokio::select! {
            _ = terminate.recv() => {}
            _ = interrupt.recv() => {}
        }
        let _ = shutdown_tx.send(true);
    });
    let mut connections = tokio::task::JoinSet::new();

    loop {
        tokio::select! {
            accepted = listener.accept() => {
                let (stream, _) = match accepted {
                    Ok(accepted) => accepted,
                    Err(error) if matches!(
                        error.kind(),
                        io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock | io::ErrorKind::ConnectionAborted
                    ) => {
                        tokio::time::sleep(Duration::from_millis(10)).await;
                        continue;
                    }
                    Err(error) => return Err(ServerError::Accept(error)),
                };
                let Ok(credentials) = rustix::net::sockopt::socket_peercred(stream.as_fd()) else {
                    continue;
                };
                let peer = PeerCredentials {
                    pid: credentials.pid.as_raw_nonzero().get(),
                    uid: credentials.uid.as_raw(),
                    gid: credentials.gid.as_raw(),
                };
                let uid = peer.uid;
                if !peer_is_authorized(config.context, uid) {
                    continue;
                }
                if !registry.admission_allowed(uid) {
                    continue;
                }
                let Some(connection_permit) = registry.connection(uid) else {
                    let _ = registry.rejection(uid);
                    continue;
                };
                let connection = Connection {
                    stream,
                    peer,
                    config: config.clone(),
                    epoch: Arc::clone(&epoch),
                    registry: registry.clone(),
                    admission: connection_permit,
                    shutdown: shutdown_rx.clone(),
                };
                connections.spawn(async move { connection.run().await; });
            }
            changed = shutdown_rx.changed() => {
                if changed.is_err() || *shutdown_rx.borrow() { break; }
            }
            joined = connections.join_next(), if !connections.is_empty() => {
                let _ = joined;
            }
        }
    }
    let drain = async { while connections.join_next().await.is_some() {} };
    if tokio::time::timeout(Duration::from_secs(3), drain)
        .await
        .is_err()
    {
        connections.abort_all();
    }
    Ok(())
}

struct Connection {
    stream: UnixStream,
    peer: PeerCredentials,
    config: ServerConfig,
    epoch: Arc<WorkerEpoch>,
    registry: AdmissionRegistry,
    admission: AdmissionPermit,
    shutdown: watch::Receiver<bool>,
}

impl Connection {
    #[allow(clippy::too_many_lines)]
    async fn run(self) {
        let Self {
            stream,
            peer,
            config,
            epoch,
            registry,
            admission,
            mut shutdown,
        } = self;
        let _admission = admission;
        let uid = peer.uid;
        if !registry.admission_allowed(uid) {
            return;
        }
        let Some(handshake_permit) = registry.handshake(uid) else {
            let _ = registry.rejection(uid);
            return;
        };
        let (mut reader, mut direct_writer) = tokio::io::split(stream);
        let handshake_deadline = tokio::time::Instant::now() + HANDSHAKE_TIMEOUT;
        let hello = tokio::time::timeout_at(
            handshake_deadline,
            read_frame::<_, ClientHandshakeFrame>(&mut reader),
        )
        .await;
        let client_hello = match hello {
            Ok(Ok(ClientHandshakeFrame::ClientHello(client_hello))) => client_hello,
            Ok(Err(error)) => {
                if registry.rejection(uid) {
                    let frame = ServerHandshakeFrame::ProtocolError(frame_error(&error));
                    let _ = tokio::time::timeout_at(
                        handshake_deadline,
                        write_frame(&mut direct_writer, &frame),
                    )
                    .await;
                }
                return;
            }
            Err(_) => {
                let _ = registry.rejection(uid);
                return;
            }
        };
        let Ok(connection_id) = ConnectionIdentifier::from_uuid(uuid::Uuid::now_v7()) else {
            return;
        };
        let Ok(server_time_ms) = epoch.logical_now() else {
            return;
        };
        let handshake_config = ServerHandshakeConfig {
            protocol: config.protocol,
            software_version: config.software_version,
            context: config.context,
            capabilities: config.capabilities,
            limits: config.limits,
            manifest: config.manifest,
            worker_epoch: epoch.identifier(),
            server_time_ms,
            server_connection_id: connection_id,
        };
        let server_hello = match negotiate_handshake(&client_hello, &handshake_config) {
            Ok(hello) => hello,
            Err(error) => {
                if registry.rejection(uid) {
                    let frame = ServerHandshakeFrame::ProtocolError(handshake_error(&error));
                    let _ = tokio::time::timeout_at(
                        handshake_deadline,
                        write_frame(&mut direct_writer, &frame),
                    )
                    .await;
                }
                return;
            }
        };
        let Some(_principal_byte_limit) = registry.principal_byte_limit(
            uid,
            usize::try_from(server_hello.effective_limits.buffered_response_bytes())
                .unwrap_or(usize::MAX),
        ) else {
            let _ = registry.rejection(uid);
            return;
        };
        if !matches!(
            tokio::time::timeout_at(
                handshake_deadline,
                write_frame(
                    &mut direct_writer,
                    &ServerHandshakeFrame::ServerHello(server_hello.clone()),
                ),
            )
            .await,
            Ok(Ok(()))
        ) {
            return;
        }
        drop(handshake_permit);

        let (outbound_tx, outbound_rx) = mpsc::channel(OUTBOUND_QUEUE_ITEMS);
        let (connection_close_tx, connection_close_rx) = watch::channel(false);
        let writer_registry = registry.clone();
        let mut writer = tokio::spawn(writer_loop(direct_writer, outbound_rx, connection_close_rx));
        let active = Arc::new(Mutex::new(BTreeMap::new()));
        let request_slots = Arc::new(Semaphore::new(
            usize::try_from(server_hello.effective_limits.concurrent_requests()).unwrap_or(1),
        ));
        let stream_slots = Arc::new(Semaphore::new(
            usize::try_from(server_hello.effective_limits.active_streams()).unwrap_or(1),
        ));
        let connection_buffer = ConnectionBuffer::new(
            usize::try_from(server_hello.effective_limits.buffered_response_bytes()).unwrap_or(1),
        );

        let mut shutting_down = false;
        loop {
            tokio::select! {
                biased;
                result = &mut writer => {
                    let _ = result;
                    break;
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        shutting_down = true;
                        break;
                    }
                }
                received = read_frame_with_timestamp::<_, ClientFrame>(&mut reader) => {
                    let Ok(received) = received else { break; };
                    let received_at = received.received_at;
                    let frame = received.frame;
                    match frame {
                        ClientFrame::Request(request) => spawn_request(RequestContext {
                            request,
                            received_at,
                            expected_connection: connection_id,
                            peer,
                            principal: PrincipalKey { target: config.context.target(), uid },
                            limits: server_hello.effective_limits,
                            epoch: epoch.clone(),
                            dispatcher: config.dispatcher.clone(),
                            registry: writer_registry.clone(),
                            request_slots: request_slots.clone(),
                            stream_slots: stream_slots.clone(),
                            active: active.clone(),
                            connection_buffer: connection_buffer.clone(),
                            outbound: outbound_tx.clone(),
                            connection_close: connection_close_tx.clone(),
                            completion: Arc::new(Notify::new()),
                            terminal_lease: Arc::new(Mutex::new(None)),
                            request_deadline: Arc::new(Mutex::new(None)),
                            shutdown: shutdown.clone(),
                        }).await,
                        ClientFrame::StreamAck(control) => {
                            if let Err(code) = control_request(
                                control.server_connection_id,
                                control.request_id,
                                Some(control.sequence),
                                connection_id,
                                &active,
                                false,
                            ) {
                                queue_control_error(
                                    uid,
                                    ConnectionEpoch {
                                        connection_id,
                                        worker_epoch: epoch.identifier(),
                                    },
                                    control.request_id,
                                    code,
                                    &writer_registry,
                                    &connection_buffer,
                                    (&outbound_tx, &connection_close_tx),
                                )
                                .await;
                            }
                        }
                        ClientFrame::Cancel(control) => {
                            if let Err(code) = control_request(
                                control.server_connection_id,
                                control.request_id,
                                None,
                                connection_id,
                                &active,
                                true,
                            ) {
                                queue_control_error(
                                    uid,
                                    ConnectionEpoch {
                                        connection_id,
                                        worker_epoch: epoch.identifier(),
                                    },
                                    control.request_id,
                                    code,
                                    &writer_registry,
                                    &connection_buffer,
                                    (&outbound_tx, &connection_close_tx),
                                )
                                .await;
                            }
                        }
                    }
                }
            }
        }
        cancel_all(&active, shutting_down);
        drop(outbound_tx);
        if !writer.is_finished()
            && tokio::time::timeout(Duration::from_secs(2), &mut writer)
                .await
                .is_err()
        {
            writer.abort();
            let _ = writer.await;
        }
    }
}

#[cfg(feature = "worker-test-fixtures")]
#[derive(Clone)]
struct StreamProgress {
    produced: Arc<AtomicU64>,
    delivered: Arc<AtomicU64>,
    control: watch::Sender<ControlState>,
}

#[derive(Clone)]
struct DeliveryPublication {
    delivered: Arc<AtomicU64>,
    control: watch::Sender<ControlState>,
    sequence: u64,
}

#[derive(Clone)]
struct OutputGuard {
    deadline_at: tokio::time::Instant,
    control: watch::Receiver<ControlState>,
    shutdown: watch::Receiver<bool>,
    publication: Option<DeliveryPublication>,
}

struct RequestLease {
    active: ActiveMap,
    request_id: crate::protocol::RequestIdentifier,
    completion: Arc<Notify>,
}

impl Drop for RequestLease {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(&self.request_id);
        }
        self.completion.notify_one();
    }
}

enum WriteOutcome {
    Written,
    Skipped,
    Failed,
}

enum Outbound {
    Frame {
        bytes: Vec<u8>,
        principal_permit: AdmissionPermit,
        connection_permit: ConnectionBufferPermit,
        guard: Option<OutputGuard>,
        delivery_deadline: tokio::time::Instant,
        terminal_lease: Option<RequestLease>,
    },
    #[cfg_attr(not(feature = "worker-test-fixtures"), allow(dead_code))]
    Barrier { completion: oneshot::Sender<()> },
}

#[allow(clippy::too_many_lines)]
async fn writer_loop(
    mut writer: WriteHalf<UnixStream>,
    mut receiver: mpsc::Receiver<Outbound>,
    mut connection_close: watch::Receiver<bool>,
) {
    loop {
        let outbound = tokio::select! {
            biased;
            changed = connection_close.changed() => {
                let _ = changed;
                break;
            }
            outbound = receiver.recv() => {
                let Some(outbound) = outbound else { break; };
                outbound
            }
        };
        let Outbound::Frame {
            bytes,
            principal_permit,
            connection_permit,
            guard,
            delivery_deadline,
            terminal_lease,
        } = outbound
        else {
            let Outbound::Barrier { completion } = outbound else {
                unreachable!();
            };
            let _ = completion.send(());
            continue;
        };
        let is_terminal = terminal_lease.is_some();
        let mut write_guard = guard.clone();
        let guard_expired = write_guard.as_ref().is_some_and(|guard| {
            tokio::time::Instant::now() >= guard.deadline_at
                || *guard.shutdown.borrow()
                || guard.control.borrow().cancelled
                || guard.control.borrow().worker_shutdown
        });
        let write = tokio::time::timeout_at(delivery_deadline, writer.write_all(&bytes));
        tokio::pin!(write);
        let outcome = if tokio::time::Instant::now() >= delivery_deadline {
            WriteOutcome::Failed
        } else if guard_expired {
            WriteOutcome::Skipped
        } else if let Some(guard) = write_guard.as_mut() {
            let mut control_open = true;
            loop {
                tokio::select! {
                    biased;
                    changed = connection_close.changed() => {
                        let _ = changed;
                        break WriteOutcome::Failed;
                    }
                    () = tokio::time::sleep_until(guard.deadline_at) => {
                        break WriteOutcome::Failed;
                    }
                    changed = guard.shutdown.changed() => {
                        let _ = changed;
                        break WriteOutcome::Failed;
                    }
                    changed = guard.control.changed(), if control_open => {
                        if changed.is_err() {
                            control_open = false;
                        } else {
                            let state = *guard.control.borrow();
                            if state.cancelled || state.worker_shutdown {
                                break WriteOutcome::Failed;
                            }
                        }
                    }
                    result = &mut write => break if matches!(result, Ok(Ok(()))) {
                        WriteOutcome::Written
                    } else {
                        WriteOutcome::Failed
                    },
                }
            }
        } else {
            tokio::select! {
                biased;
                changed = connection_close.changed() => {
                    let _ = changed;
                    WriteOutcome::Failed
                }
                () = tokio::time::sleep_until(delivery_deadline) => WriteOutcome::Failed,
                result = &mut write => if matches!(result, Ok(Ok(()))) {
                    WriteOutcome::Written
                } else {
                    WriteOutcome::Failed
                },
            }
        };
        if matches!(outcome, WriteOutcome::Written) {
            if let Some(publication) = guard
                .as_ref()
                .and_then(|output_guard| output_guard.publication.as_ref())
            {
                publication
                    .delivered
                    .store(publication.sequence, Ordering::Release);
                publication.control.send_modify(|state| {
                    if state.pending_acknowledgement != 0
                        && state.pending_acknowledgement <= publication.sequence
                    {
                        state.acknowledged = state.acknowledged.max(state.pending_acknowledgement);
                        state.pending_acknowledgement = 0;
                    }
                });
            }
        }
        drop(terminal_lease);
        drop(connection_permit);
        drop(principal_permit);
        if matches!(outcome, WriteOutcome::Failed)
            || (is_terminal && matches!(outcome, WriteOutcome::Skipped))
        {
            break;
        }
    }
    let _ = writer.shutdown().await;
}

#[derive(Debug, Clone, Copy)]
struct ControlState {
    #[cfg_attr(not(feature = "worker-test-fixtures"), allow(dead_code))]
    cancelled: bool,
    #[cfg_attr(not(feature = "worker-test-fixtures"), allow(dead_code))]
    worker_shutdown: bool,
    acknowledged: u64,
    pending_acknowledgement: u64,
}

struct ActiveRequest {
    control: watch::Sender<ControlState>,
    produced: Arc<AtomicU64>,
    delivered: Arc<AtomicU64>,
    completion: Arc<Notify>,
    stream: bool,
}

type ActiveMap = Arc<Mutex<BTreeMap<crate::protocol::RequestIdentifier, ActiveRequest>>>;

fn control_request(
    server_connection_id: ConnectionIdentifier,
    request_id: crate::protocol::RequestIdentifier,
    sequence: Option<u64>,
    expected_connection: ConnectionIdentifier,
    active: &ActiveMap,
    cancel: bool,
) -> Result<(), WorkerErrorCode> {
    if server_connection_id != expected_connection {
        return Err(WorkerErrorCode::ConnectionMismatch);
    }
    let (control, produced, delivered, stream) = {
        let active = active
            .lock()
            .map_err(|_| WorkerErrorCode::RequestNotFound)?;
        let Some(request) = active.get(&request_id) else {
            return Err(WorkerErrorCode::RequestNotFound);
        };
        (
            request.control.clone(),
            request.produced.clone(),
            request.delivered.clone(),
            request.stream,
        )
    };
    let current = *control.borrow();
    if cancel {
        control.send_modify(|state| state.cancelled = true);
        return Ok(());
    }
    let Some(sequence) = sequence else {
        return Err(WorkerErrorCode::InvalidAcknowledgement);
    };
    let acknowledgement_floor = current.acknowledged.max(current.pending_acknowledgement);
    if !stream {
        return Err(WorkerErrorCode::InvalidAcknowledgement);
    }
    if sequence < acknowledgement_floor || sequence > produced.load(Ordering::Acquire) {
        control.send_modify(|state| state.cancelled = true);
        return Err(WorkerErrorCode::InvalidAcknowledgement);
    }
    if sequence <= delivered.load(Ordering::Acquire) {
        control.send_modify(|state| state.acknowledged = state.acknowledged.max(sequence));
    } else {
        control.send_modify(|state| {
            state.pending_acknowledgement = state.pending_acknowledgement.max(sequence);
        });
        if sequence <= delivered.load(Ordering::Acquire) {
            control.send_modify(|state| {
                state.acknowledged = state.acknowledged.max(sequence);
                if state.pending_acknowledgement <= state.acknowledged {
                    state.pending_acknowledgement = 0;
                }
            });
        }
    }
    Ok(())
}

fn cancel_all(active: &ActiveMap, worker_shutdown: bool) {
    if let Ok(active) = active.lock() {
        for request in active.values() {
            request.control.send_modify(|state| {
                state.cancelled = !worker_shutdown;
                state.worker_shutdown = worker_shutdown;
            });
            request.completion.notify_one();
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ConnectionEpoch {
    connection_id: ConnectionIdentifier,
    worker_epoch: ConnectionIdentifier,
}

struct RequestContext {
    request: Request,
    received_at: tokio::time::Instant,
    expected_connection: ConnectionIdentifier,
    peer: PeerCredentials,
    principal: PrincipalKey,
    limits: EffectiveLimits,
    epoch: Arc<WorkerEpoch>,
    dispatcher: Arc<dyn SemanticDispatcher>,
    registry: AdmissionRegistry,
    request_slots: Arc<Semaphore>,
    #[cfg_attr(not(feature = "worker-test-fixtures"), allow(dead_code))]
    stream_slots: Arc<Semaphore>,
    active: ActiveMap,
    connection_buffer: ConnectionBuffer,
    outbound: mpsc::Sender<Outbound>,
    connection_close: watch::Sender<bool>,
    completion: Arc<Notify>,
    terminal_lease: Arc<Mutex<Option<RequestLease>>>,
    request_deadline: Arc<Mutex<Option<tokio::time::Instant>>>,
    shutdown: watch::Receiver<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OperationDeadlineClass {
    Unary,
    #[cfg_attr(not(feature = "worker-test-fixtures"), allow(dead_code))]
    Lifecycle,
}

const fn operation_deadline_class(operation: &SemanticRequest) -> OperationDeadlineClass {
    match operation {
        SemanticRequest::Reserved => OperationDeadlineClass::Unary,
        #[cfg(feature = "worker-test-fixtures")]
        SemanticRequest::MockUnary { .. } | SemanticRequest::MockStream { .. } => {
            OperationDeadlineClass::Unary
        }
        #[cfg(feature = "worker-test-fixtures")]
        SemanticRequest::MockLifecycle { .. } => OperationDeadlineClass::Lifecycle,
    }
}

#[cfg(feature = "worker-test-fixtures")]
const fn is_stream_operation(operation: &SemanticRequest) -> bool {
    matches!(operation, SemanticRequest::MockStream { .. })
}

#[cfg(not(feature = "worker-test-fixtures"))]
const fn is_stream_operation(_operation: &SemanticRequest) -> bool {
    false
}

#[allow(clippy::too_many_lines)]
async fn spawn_request(context: RequestContext) {
    if !context.registry.admission_allowed(context.peer.uid) {
        let _ = context.connection_close.send(true);
        return;
    }
    if context.request.server_connection_id != context.expected_connection {
        send_error(
            &context,
            WorkerErrorCode::ConnectionMismatch,
            "connection identifier mismatch",
        )
        .await;
        return;
    }
    let (control_tx, control_rx) = watch::channel(ControlState {
        cancelled: false,
        worker_shutdown: false,
        acknowledged: 0,
        pending_acknowledgement: 0,
    });
    let delivery_control = control_tx.clone();
    let produced = Arc::new(AtomicU64::new(0));
    let delivered = Arc::new(AtomicU64::new(0));
    let stream = is_stream_operation(&context.request.operation);
    let identifier_reserved = {
        let Ok(mut active) = context.active.lock() else {
            return;
        };
        if let std::collections::btree_map::Entry::Vacant(entry) =
            active.entry(context.request.request_id)
        {
            entry.insert(ActiveRequest {
                control: control_tx.clone(),
                produced: produced.clone(),
                delivered: delivered.clone(),
                completion: context.completion.clone(),
                stream,
            });
            true
        } else {
            false
        }
    };
    if !identifier_reserved {
        send_error(
            &context,
            WorkerErrorCode::RequestConflict,
            "request identifier is already active",
        )
        .await;
        return;
    }
    let deadline_maximum = match operation_deadline_class(&context.request.operation) {
        OperationDeadlineClass::Unary => context.limits.unary_deadline_ms(),
        OperationDeadlineClass::Lifecycle => context.limits.lifecycle_deadline_ms(),
    };
    let deadline_ms = context.request.deadline_ms.unwrap_or(deadline_maximum);
    if deadline_ms == 0 || deadline_ms > deadline_maximum {
        if let Ok(mut active) = context.active.lock() {
            active.remove(&context.request.request_id);
        }
        send_error(
            &context,
            WorkerErrorCode::Deadline,
            "request deadline is invalid",
        )
        .await;
        return;
    }
    let deadline_at = context.received_at + Duration::from_millis(deadline_ms);
    {
        let Ok(mut request_deadline) = context.request_deadline.lock() else {
            if let Ok(mut active) = context.active.lock() {
                active.remove(&context.request.request_id);
            }
            let _ = context.connection_close.send(true);
            return;
        };
        *request_deadline = Some(deadline_at);
    }
    let Ok(connection_permit) = context.request_slots.clone().try_acquire_owned() else {
        if let Ok(mut active) = context.active.lock() {
            active.remove(&context.request.request_id);
        }
        send_error(
            &context,
            WorkerErrorCode::Overloaded,
            "connection request limit reached",
        )
        .await;
        return;
    };
    let Some(principal_permit) = context.registry.request(context.peer.uid) else {
        if let Ok(mut active) = context.active.lock() {
            active.remove(&context.request.request_id);
        }
        send_error(
            &context,
            WorkerErrorCode::Overloaded,
            "worker request limit reached",
        )
        .await;
        return;
    };
    let lease = RequestLease {
        active: context.active.clone(),
        request_id: context.request.request_id,
        completion: context.completion.clone(),
    };
    let Ok(mut terminal_lease) = context.terminal_lease.lock() else {
        if let Ok(mut active) = context.active.lock() {
            active.remove(&context.request.request_id);
        }
        return;
    };
    *terminal_lease = Some(lease);
    drop(terminal_lease);
    tokio::spawn(async move {
        let completion = context.completion.clone();
        run_request(
            context,
            control_rx,
            delivery_control,
            produced,
            delivered,
            deadline_at,
        )
        .await;
        completion.notified().await;
        drop(principal_permit);
        drop(connection_permit);
    });
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DispatchInterruption {
    Deadline,
    Cancelled,
    WorkerShutdown,
}

async fn await_dispatch(
    dispatcher: &dyn SemanticDispatcher,
    context: &DispatchContext,
    operation: &SemanticRequest,
    deadline_at: tokio::time::Instant,
    control: &mut watch::Receiver<ControlState>,
    shutdown: &mut watch::Receiver<bool>,
) -> Result<DispatchPlan, DispatchInterruption> {
    if tokio::time::Instant::now() >= deadline_at {
        return Err(DispatchInterruption::Deadline);
    }
    let dispatch = dispatcher.dispatch(context, operation);
    tokio::pin!(dispatch);
    loop {
        tokio::select! {
            biased;
            () = tokio::time::sleep_until(deadline_at) => {
                return Err(DispatchInterruption::Deadline);
            }
            changed = shutdown.changed() => {
                let _ = changed;
                return Err(DispatchInterruption::WorkerShutdown);
            }
            changed = control.changed() => {
                let _ = changed;
                if control.borrow().worker_shutdown {
                    return Err(DispatchInterruption::WorkerShutdown);
                }
                if control.borrow().cancelled {
                    return Err(DispatchInterruption::Cancelled);
                }
            }
            plan = &mut dispatch => {
                return if tokio::time::Instant::now() >= deadline_at {
                    Err(DispatchInterruption::Deadline)
                } else {
                    Ok(plan)
                };
            }
        }
    }
}

async fn run_request(
    context: RequestContext,
    #[allow(unused_mut)] mut control: watch::Receiver<ControlState>,
    delivery_control: watch::Sender<ControlState>,
    produced: Arc<AtomicU64>,
    delivered: Arc<AtomicU64>,
    deadline_at: tokio::time::Instant,
) {
    #[cfg(not(feature = "worker-test-fixtures"))]
    let _ = (
        &control,
        &delivery_control,
        &produced,
        &delivered,
        deadline_at,
    );
    let dispatch_context = DispatchContext {
        principal: context.principal,
        peer: context.peer,
        worker_epoch: context.epoch.identifier(),
        connection_id: context.expected_connection,
        request_id: context.request.request_id,
    };
    let mut dispatch_shutdown = context.shutdown.clone();
    let plan = match await_dispatch(
        context.dispatcher.as_ref(),
        &dispatch_context,
        &context.request.operation,
        deadline_at,
        &mut control,
        &mut dispatch_shutdown,
    )
    .await
    {
        Ok(plan) => plan,
        Err(DispatchInterruption::Deadline) => {
            queue_request_error(
                &context,
                WorkerErrorCode::Deadline,
                "request deadline elapsed",
            )
            .await;
            return;
        }
        Err(DispatchInterruption::Cancelled) => {
            queue_request_error(&context, WorkerErrorCode::Cancelled, "request cancelled").await;
            return;
        }
        Err(DispatchInterruption::WorkerShutdown) => {
            queue_request_error(&context, WorkerErrorCode::WorkerShutdown, "worker shutdown").await;
            return;
        }
    };
    match plan {
        DispatchPlan::Unsupported => {
            queue_frame(
                &context,
                ServerFrame::Response(Response {
                    server_connection_id: context.expected_connection,
                    request_id: context.request.request_id,
                    result: worker_error(
                        context.epoch.identifier(),
                        WorkerErrorCode::Unsupported,
                        "semantic operation is unavailable",
                        OperationPhase::Dispatch,
                    ),
                }),
                None,
            )
            .await;
        }
        #[cfg(feature = "worker-test-fixtures")]
        DispatchPlan::MockUnary { delay_ms } => {
            run_mock_unary(&context, &mut control, deadline_at, delay_ms).await;
        }
        #[cfg(feature = "worker-test-fixtures")]
        DispatchPlan::MockStream { items, interval_ms } => {
            run_mock_stream(
                context,
                &mut control,
                StreamProgress {
                    produced,
                    delivered,
                    control: delivery_control,
                },
                deadline_at,
                items,
                interval_ms,
            )
            .await;
        }
    }
}

#[cfg(feature = "worker-test-fixtures")]
async fn run_mock_unary(
    context: &RequestContext,
    control: &mut watch::Receiver<ControlState>,
    deadline_at: tokio::time::Instant,
    delay_ms: u64,
) {
    let mut request_shutdown = context.shutdown.clone();
    let completion_at = tokio::time::Instant::now()
        .checked_add(Duration::from_millis(delay_ms))
        .unwrap_or(deadline_at)
        .min(deadline_at);
    let result = tokio::select! {
        biased;
        () = tokio::time::sleep_until(deadline_at) => worker_error(context.epoch.identifier(), WorkerErrorCode::Deadline, "request deadline elapsed", OperationPhase::Execution),
        changed = control.changed() => {
            let _ = changed;
            let state = *control.borrow();
            if state.worker_shutdown {
                worker_error(context.epoch.identifier(), WorkerErrorCode::WorkerShutdown, "worker shutdown", OperationPhase::Execution)
            } else {
                worker_error(context.epoch.identifier(), WorkerErrorCode::Cancelled, "request cancelled", OperationPhase::Execution)
            }
        }
        changed = request_shutdown.changed() => {
            let _ = changed;
            worker_error(context.epoch.identifier(), WorkerErrorCode::WorkerShutdown, "worker shutdown", OperationPhase::Execution)
        }
        () = tokio::time::sleep_until(completion_at) => {
            if tokio::time::Instant::now() >= deadline_at {
                worker_error(context.epoch.identifier(), WorkerErrorCode::Deadline, "request deadline elapsed", OperationPhase::Execution)
            } else {
                ResponseResult::MockComplete
            }
        },
    };
    queue_frame(
        context,
        ServerFrame::Response(Response {
            server_connection_id: context.expected_connection,
            request_id: context.request.request_id,
            result,
        }),
        None,
    )
    .await;
}

#[cfg(feature = "worker-test-fixtures")]
#[allow(clippy::too_many_lines)]
async fn run_mock_stream(
    context: RequestContext,
    control: &mut watch::Receiver<ControlState>,
    progress: StreamProgress,
    deadline_at: tokio::time::Instant,
    items: u32,
    interval_ms: u64,
) {
    let Ok(connection_stream) = context.stream_slots.clone().try_acquire_owned() else {
        send_error(
            &context,
            WorkerErrorCode::Overloaded,
            "connection stream limit reached",
        )
        .await;
        return;
    };
    let Some(principal_stream) = context.registry.stream(context.peer.uid) else {
        send_error(
            &context,
            WorkerErrorCode::Overloaded,
            "worker stream limit reached",
        )
        .await;
        return;
    };
    let mut stream_shutdown = context.shutdown.clone();
    let mut reason = StreamEndReason::Completed;
    let mut sequence = 0_u64;
    'items: for value in 1..=items {
        let mut stalled_at = None;
        while sequence.saturating_sub(control.borrow().acknowledged)
            >= u64::from(context.limits.unacknowledged_stream_items())
        {
            if stalled_at.is_none()
                && progress.delivered.load(Ordering::Acquire) > control.borrow().acknowledged
            {
                stalled_at = Some(tokio::time::Instant::now() + Duration::from_secs(1));
            }
            tokio::select! {
                biased;
                () = tokio::time::sleep_until(deadline_at) => {
                    reason = StreamEndReason::Deadline;
                    break 'items;
                }
                () = tokio::time::sleep_until(stalled_at.unwrap_or(deadline_at)), if stalled_at.is_some() => {
                    reason = StreamEndReason::SlowConsumer;
                    break 'items;
                }
                changed = control.changed() => {
                    let _ = changed;
                    if control.borrow().worker_shutdown {
                        reason = StreamEndReason::WorkerShutdown;
                        break 'items;
                    }
                    if control.borrow().cancelled {
                        reason = StreamEndReason::Cancelled;
                        break 'items;
                    }
                }
                changed = stream_shutdown.changed() => {
                    let _ = changed;
                    reason = StreamEndReason::WorkerShutdown;
                    break 'items;
                }
            }
        }
        if reason != StreamEndReason::Completed {
            break;
        }
        if tokio::time::Instant::now() >= deadline_at {
            reason = StreamEndReason::Deadline;
            break;
        }
        if *context.shutdown.borrow() {
            reason = StreamEndReason::WorkerShutdown;
            break;
        }
        if control.borrow().worker_shutdown {
            reason = StreamEndReason::WorkerShutdown;
            break;
        }
        if control.borrow().cancelled {
            reason = StreamEndReason::Cancelled;
            break;
        }
        let item_at = tokio::time::Instant::now()
            .checked_add(Duration::from_millis(interval_ms))
            .unwrap_or(deadline_at)
            .min(deadline_at);
        loop {
            tokio::select! {
                biased;
                () = tokio::time::sleep_until(deadline_at) => {
                    reason = StreamEndReason::Deadline;
                    break 'items;
                }
                changed = stream_shutdown.changed() => {
                    let _ = changed;
                    reason = StreamEndReason::WorkerShutdown;
                    break 'items;
                }
                changed = control.changed() => {
                    let _ = changed;
                    if control.borrow().worker_shutdown {
                        reason = StreamEndReason::WorkerShutdown;
                        break 'items;
                    }
                    if control.borrow().cancelled {
                        reason = StreamEndReason::Cancelled;
                        break 'items;
                    }
                }
                () = tokio::time::sleep_until(item_at) => {
                    if tokio::time::Instant::now() >= deadline_at {
                        reason = StreamEndReason::Deadline;
                        break 'items;
                    }
                    break;
                }
            }
        }
        sequence = sequence.saturating_add(1);
        progress.produced.store(sequence, Ordering::Release);
        queue_frame(
            &context,
            ServerFrame::StreamItem(StreamItem {
                server_connection_id: context.expected_connection,
                request_id: context.request.request_id,
                worker_epoch: context.epoch.identifier(),
                sequence,
                mock_value: value,
            }),
            Some(OutputGuard {
                deadline_at,
                control: control.clone(),
                shutdown: context.shutdown.clone(),
                publication: Some(DeliveryPublication {
                    delivered: progress.delivered.clone(),
                    control: progress.control.clone(),
                    sequence,
                }),
            }),
        )
        .await;
    }
    if !await_output_barrier(&context).await {
        let _ = context.connection_close.send(true);
        return;
    }
    let final_sequence = progress.delivered.load(Ordering::Acquire);
    if reason == StreamEndReason::Completed {
        if tokio::time::Instant::now() >= deadline_at {
            reason = StreamEndReason::Deadline;
        } else if *context.shutdown.borrow() || control.borrow().worker_shutdown {
            reason = StreamEndReason::WorkerShutdown;
        } else if control.borrow().cancelled {
            reason = StreamEndReason::Cancelled;
        }
    }
    queue_frame(
        &context,
        ServerFrame::StreamEnd(StreamEnd {
            server_connection_id: context.expected_connection,
            request_id: context.request.request_id,
            worker_epoch: context.epoch.identifier(),
            reason,
            final_sequence,
        }),
        None,
    )
    .await;
    drop(principal_stream);
    drop(connection_stream);
}

#[cfg(feature = "worker-test-fixtures")]
async fn await_output_barrier(context: &RequestContext) -> bool {
    let (completion, completed) = oneshot::channel();
    let barrier = Outbound::Barrier { completion };
    let barrier_deadline = tokio::time::Instant::now() + PARTIAL_FRAME_TIMEOUT;
    if tokio::select! {
        biased;
        result = context.outbound.send(barrier) => result.is_err(),
        () = tokio::time::sleep_until(barrier_deadline) => true,
    } {
        return false;
    }
    tokio::select! {
        biased;
        result = completed => result.is_ok(),
        () = tokio::time::sleep_until(barrier_deadline) => false,
    }
}

async fn queue_request_error(
    context: &RequestContext,
    code: WorkerErrorCode,
    summary: &'static str,
) {
    #[cfg(feature = "worker-test-fixtures")]
    if is_stream_operation(&context.request.operation) {
        let reason = match code {
            WorkerErrorCode::Cancelled => Some(StreamEndReason::Cancelled),
            WorkerErrorCode::Deadline => Some(StreamEndReason::Deadline),
            WorkerErrorCode::WorkerShutdown => Some(StreamEndReason::WorkerShutdown),
            _ => None,
        };
        if let Some(reason) = reason {
            queue_frame(
                context,
                ServerFrame::StreamEnd(StreamEnd {
                    server_connection_id: context.expected_connection,
                    request_id: context.request.request_id,
                    worker_epoch: context.epoch.identifier(),
                    reason,
                    final_sequence: 0,
                }),
                None,
            )
            .await;
            return;
        }
    }
    queue_frame(
        context,
        ServerFrame::Response(Response {
            server_connection_id: context.expected_connection,
            request_id: context.request.request_id,
            result: worker_error(
                context.epoch.identifier(),
                code,
                summary,
                OperationPhase::Dispatch,
            ),
        }),
        None,
    )
    .await;
}

async fn send_error(context: &RequestContext, code: WorkerErrorCode, summary: &'static str) {
    if !context.registry.rejection(context.peer.uid) {
        let _ = context.connection_close.send(true);
        return;
    }
    queue_frame(
        context,
        ServerFrame::Response(Response {
            server_connection_id: context.expected_connection,
            request_id: context.request.request_id,
            result: worker_error(
                context.epoch.identifier(),
                code,
                summary,
                OperationPhase::Admission,
            ),
        }),
        None,
    )
    .await;
}

async fn queue_frame(context: &RequestContext, frame: ServerFrame, guard: Option<OutputGuard>) {
    let terminal_lease = if matches!(&frame, ServerFrame::Response(_) | ServerFrame::StreamEnd(_)) {
        let Ok(mut lease) = context.terminal_lease.lock() else {
            let _ = context.connection_close.send(true);
            return;
        };
        lease.take()
    } else {
        None
    };
    let now = tokio::time::Instant::now();
    let fixed_deadline = now + TERMINAL_DELIVERY_TIMEOUT;
    let request_deadline = context
        .request_deadline
        .lock()
        .ok()
        .and_then(|deadline| *deadline);
    let delivery_deadline = guard.as_ref().map_or_else(
        || {
            request_deadline
                .filter(|deadline| *deadline > now)
                .map_or(fixed_deadline, |deadline| deadline.min(fixed_deadline))
        },
        |output_guard| output_guard.deadline_at.min(fixed_deadline),
    );
    queue_encoded(
        context.peer.uid,
        &context.registry,
        &context.connection_buffer,
        (&context.outbound, &context.connection_close),
        QueuedFrame {
            frame,
            guard,
            delivery_deadline,
            terminal_lease,
        },
    )
    .await;
}

async fn queue_control_error(
    uid: u32,
    identity: ConnectionEpoch,
    request_id: crate::protocol::RequestIdentifier,
    code: WorkerErrorCode,
    registry: &AdmissionRegistry,
    connection_buffer: &ConnectionBuffer,
    output: (&mpsc::Sender<Outbound>, &watch::Sender<bool>),
) {
    if !registry.rejection(uid) {
        let _ = output.1.send(true);
        return;
    }
    queue_encoded(
        uid,
        registry,
        connection_buffer,
        output,
        QueuedFrame {
            frame: ServerFrame::ControlError(ControlError {
                server_connection_id: identity.connection_id,
                request_id,
                error: WorkerError {
                    code,
                    summary: SafeSummary::parse("request control frame is invalid")
                        .expect("fixed control summary is valid"),
                    retry: if code == WorkerErrorCode::RequestNotFound {
                        RetryClassification::AfterStateRefresh
                    } else {
                        RetryClassification::Never
                    },
                    phase: OperationPhase::Stream,
                    worker_epoch: identity.worker_epoch,
                },
            }),
            guard: None,
            delivery_deadline: tokio::time::Instant::now() + TERMINAL_DELIVERY_TIMEOUT,
            terminal_lease: None,
        },
    )
    .await;
}

struct QueuedFrame {
    frame: ServerFrame,
    guard: Option<OutputGuard>,
    delivery_deadline: tokio::time::Instant,
    terminal_lease: Option<RequestLease>,
}

async fn queue_encoded(
    uid: u32,
    registry: &AdmissionRegistry,
    connection_buffer: &ConnectionBuffer,
    output: (&mpsc::Sender<Outbound>, &watch::Sender<bool>),
    queued: QueuedFrame,
) {
    let QueuedFrame {
        frame,
        guard,
        delivery_deadline,
        terminal_lease,
    } = queued;
    let Ok(encoded_length) = encoded_frame_len(&frame, FrameDirection::ServerToClient) else {
        let _ = output.1.send(true);
        return;
    };
    let Some(connection_permit) = connection_buffer.reserve(encoded_length) else {
        let _ = output.1.send(true);
        return;
    };
    let Some(principal_permit) = registry.buffered_bytes(uid, encoded_length) else {
        let _ = output.1.send(true);
        return;
    };
    let Ok(bytes) = encode_frame_exact(&frame, FrameDirection::ServerToClient, encoded_length)
    else {
        let _ = output.1.send(true);
        return;
    };
    let mut enqueue_guard = guard.clone();
    let frame = Outbound::Frame {
        bytes,
        principal_permit,
        connection_permit,
        guard,
        delivery_deadline,
        terminal_lease,
    };
    let reserve = output.0.reserve();
    tokio::pin!(reserve);
    if let Some(guard) = enqueue_guard.as_mut() {
        let mut control_open = true;
        loop {
            tokio::select! {
                biased;
                () = tokio::time::sleep_until(delivery_deadline) => return,
                changed = guard.shutdown.changed() => {
                    let _ = changed;
                    return;
                }
                changed = guard.control.changed(), if control_open => {
                    if changed.is_err() {
                        control_open = false;
                    } else {
                        let state = *guard.control.borrow();
                        if state.cancelled || state.worker_shutdown {
                            return;
                        }
                    }
                }
                permit = &mut reserve => {
                    if let Ok(permit) = permit {
                        permit.send(frame);
                    } else {
                        let _ = output.1.send(true);
                    }
                    return;
                }
            }
        }
    } else {
        tokio::select! {
            biased;
            () = tokio::time::sleep_until(delivery_deadline) => {
                let _ = output.1.send(true);
            }
            permit = &mut reserve => {
                if let Ok(permit) = permit {
                    permit.send(frame);
                } else {
                    let _ = output.1.send(true);
                }
            }
        }
    }
}

fn worker_error(
    worker_epoch: ConnectionIdentifier,
    code: WorkerErrorCode,
    summary: &'static str,
    phase: OperationPhase,
) -> ResponseResult {
    let summary = SafeSummary::parse(summary).expect("static worker summary is valid");
    let retry = match code {
        WorkerErrorCode::Overloaded | WorkerErrorCode::WorkerShutdown => {
            RetryClassification::SameRequestWithBackoff
        }
        WorkerErrorCode::RequestConflict | WorkerErrorCode::RequestNotFound => {
            RetryClassification::AfterStateRefresh
        }
        WorkerErrorCode::ConnectionMismatch
        | WorkerErrorCode::Deadline
        | WorkerErrorCode::Cancelled
        | WorkerErrorCode::InvalidAcknowledgement
        | WorkerErrorCode::Unsupported => RetryClassification::Never,
    };
    ResponseResult::Error(WorkerError {
        code,
        summary,
        retry,
        phase,
        worker_epoch,
    })
}

fn frame_error(error: &AsyncFrameError) -> ProtocolError {
    let code = match error {
        AsyncFrameError::OversizedLength | AsyncFrameError::Encode(_) => {
            ProtocolErrorCode::LimitExceeded
        }
        AsyncFrameError::EmptyLength
        | AsyncFrameError::Io(_)
        | AsyncFrameError::Timeout
        | AsyncFrameError::Disconnected
        | AsyncFrameError::Decode => ProtocolErrorCode::Malformed,
    };
    ProtocolError {
        code,
        summary: SafeSummary::parse("initial protocol frame is invalid")
            .expect("static protocol summary is valid"),
    }
}

fn handshake_error(error: &HandshakeError) -> ProtocolError {
    let code = match error {
        HandshakeError::UnsupportedVersion | HandshakeError::InvalidServerVersion => {
            ProtocolErrorCode::UnsupportedVersion
        }
        HandshakeError::UnsupportedCapability(_) | HandshakeError::MissingServerCapability(_) => {
            ProtocolErrorCode::UnsupportedCapability
        }
        HandshakeError::InvalidServerLimits => ProtocolErrorCode::LimitExceeded,
    };
    ProtocolError {
        code,
        summary: SafeSummary::parse("handshake negotiation failed")
            .expect("static handshake summary is valid"),
    }
}

#[cfg(all(test, feature = "worker-test-fixtures"))]
mod tests {
    use std::future::Future;
    use std::pin::Pin;

    use crate::protocol::{ManagerKind, RequestIdentifier, WorkerTarget};

    use super::super::limits::WorkerLimits;
    use super::*;

    #[derive(Debug)]
    struct StuckDispatcher;

    impl SemanticDispatcher for StuckDispatcher {
        fn dispatch<'a>(
            &'a self,
            _context: &'a DispatchContext,
            _request: &'a SemanticRequest,
        ) -> Pin<Box<dyn Future<Output = DispatchPlan> + Send + 'a>> {
            Box::pin(std::future::pending())
        }
    }

    #[derive(Debug)]
    struct ReadyDispatcher;

    impl SemanticDispatcher for ReadyDispatcher {
        fn dispatch<'a>(
            &'a self,
            _context: &'a DispatchContext,
            _request: &'a SemanticRequest,
        ) -> Pin<Box<dyn Future<Output = DispatchPlan> + Send + 'a>> {
            Box::pin(async { DispatchPlan::Unsupported })
        }
    }

    #[test]
    fn user_peer_authorization_requires_exact_kernel_uid() {
        let user = WorkerContext::new(WorkerTarget::User, 1000, ManagerKind::User).unwrap();
        let system = WorkerContext::new(WorkerTarget::System, 0, ManagerKind::System).unwrap();

        assert!(peer_is_authorized(user, 1000));
        assert!(!peer_is_authorized(user, 1001));
        assert!(peer_is_authorized(system, 1001));
    }

    #[test]
    fn acknowledgement_cannot_advance_beyond_written_sequence() {
        let connection_id =
            ConnectionIdentifier::parse("018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20").unwrap();
        let request_id = RequestIdentifier::new(1).unwrap();
        let (control, _receiver) = watch::channel(ControlState {
            cancelled: false,
            worker_shutdown: false,
            acknowledged: 0,
            pending_acknowledgement: 0,
        });
        let active = Arc::new(Mutex::new(BTreeMap::from([(
            request_id,
            ActiveRequest {
                control,
                produced: Arc::new(AtomicU64::new(0)),
                delivered: Arc::new(AtomicU64::new(0)),
                completion: Arc::new(Notify::new()),
                stream: true,
            },
        )])));

        let result = control_request(
            connection_id,
            request_id,
            Some(1),
            connection_id,
            &active,
            false,
        );

        assert_eq!(result, Err(WorkerErrorCode::InvalidAcknowledgement));
    }

    #[test]
    fn speculative_acknowledgement_defers_without_blocking_cancellation() {
        let connection_id =
            ConnectionIdentifier::parse("018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20").unwrap();
        let request_id = RequestIdentifier::new(1).unwrap();
        let (control, receiver) = watch::channel(ControlState {
            cancelled: false,
            worker_shutdown: false,
            acknowledged: 0,
            pending_acknowledgement: 0,
        });
        let active = Arc::new(Mutex::new(BTreeMap::from([(
            request_id,
            ActiveRequest {
                control,
                produced: Arc::new(AtomicU64::new(1)),
                delivered: Arc::new(AtomicU64::new(0)),
                completion: Arc::new(Notify::new()),
                stream: true,
            },
        )])));

        assert_eq!(
            control_request(
                connection_id,
                request_id,
                Some(1),
                connection_id,
                &active,
                false,
            ),
            Ok(())
        );
        assert_eq!(receiver.borrow().pending_acknowledgement, 1);
        assert_eq!(
            control_request(
                connection_id,
                request_id,
                None,
                connection_id,
                &active,
                true,
            ),
            Ok(())
        );
        assert!(receiver.borrow().cancelled);
    }

    #[tokio::test]
    async fn writer_holds_byte_permits_until_blocked_write_terminates() {
        let registry = AdmissionRegistry::default();
        let connection_buffer = ConnectionBuffer::new(WorkerLimits::BUFFERED_BYTES_PER_PRINCIPAL);
        let principal_permit = registry
            .buffered_bytes(1000, WorkerLimits::BUFFERED_BYTES_PER_PRINCIPAL)
            .unwrap();
        let connection_permit = connection_buffer
            .reserve(WorkerLimits::BUFFERED_BYTES_PER_PRINCIPAL)
            .unwrap();
        let (server, _client) = UnixStream::pair().unwrap();
        let (_reader, writer) = tokio::io::split(server);
        let (outbound_tx, outbound_rx) = mpsc::channel(1);
        let (close_tx, close_rx) = watch::channel(false);
        let writer_task = tokio::spawn(writer_loop(writer, outbound_rx, close_rx));
        outbound_tx
            .send(Outbound::Frame {
                bytes: vec![0_u8; WorkerLimits::BUFFERED_BYTES_PER_PRINCIPAL],
                principal_permit,
                connection_permit,
                guard: None,
                delivery_deadline: tokio::time::Instant::now() + PARTIAL_FRAME_TIMEOUT,
                terminal_lease: None,
            })
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_millis(10)).await;

        assert!(registry.buffered_bytes(1000, 1).is_none());
        assert!(connection_buffer.reserve(1).is_none());

        close_tx.send(true).unwrap();
        writer_task.await.unwrap();
        assert!(registry.buffered_bytes(1000, 1).is_some());
        assert!(connection_buffer.reserve(1).is_some());
    }

    #[tokio::test]
    async fn dropped_terminal_output_finalizes_request_lease() {
        let registry = AdmissionRegistry::default();
        let connection_buffer = ConnectionBuffer::new(1);
        let request_id = RequestIdentifier::new(1).unwrap();
        let completion = Arc::new(Notify::new());
        let (control, _receiver) = watch::channel(ControlState {
            cancelled: false,
            worker_shutdown: false,
            acknowledged: 0,
            pending_acknowledgement: 0,
        });
        let active = Arc::new(Mutex::new(BTreeMap::from([(
            request_id,
            ActiveRequest {
                control,
                produced: Arc::new(AtomicU64::new(0)),
                delivered: Arc::new(AtomicU64::new(0)),
                completion: completion.clone(),
                stream: false,
            },
        )])));
        let lease = RequestLease {
            active: active.clone(),
            request_id,
            completion: completion.clone(),
        };
        assert!(active.lock().unwrap().contains_key(&request_id));
        let (outbound_tx, outbound_rx) = mpsc::channel(1);
        outbound_tx
            .send(Outbound::Frame {
                bytes: vec![0],
                principal_permit: registry.buffered_bytes(1000, 1).unwrap(),
                connection_permit: connection_buffer.reserve(1).unwrap(),
                guard: None,
                delivery_deadline: tokio::time::Instant::now() + PARTIAL_FRAME_TIMEOUT,
                terminal_lease: Some(lease),
            })
            .await
            .unwrap();

        drop(outbound_rx);
        tokio::time::timeout(Duration::from_millis(100), completion.notified())
            .await
            .unwrap();

        assert!(!active.lock().unwrap().contains_key(&request_id));
        assert!(registry.buffered_bytes(1000, 1).is_some());
        assert!(connection_buffer.reserve(1).is_some());
    }

    #[tokio::test]
    async fn skipped_terminal_frame_closes_instead_of_draining_later_output() {
        let registry = AdmissionRegistry::default();
        let connection_buffer = ConnectionBuffer::new(1);
        let (server, _client) = UnixStream::pair().unwrap();
        let (_reader, writer) = tokio::io::split(server);
        let (outbound_tx, outbound_rx) = mpsc::channel(2);
        let (_close_tx, close_rx) = watch::channel(false);
        let (_control_tx, control) = watch::channel(ControlState {
            cancelled: false,
            worker_shutdown: false,
            acknowledged: 0,
            pending_acknowledgement: 0,
        });
        let (_shutdown_tx, shutdown) = watch::channel(false);
        let request_id = RequestIdentifier::new(1).unwrap();
        let writer_task = tokio::spawn(writer_loop(writer, outbound_rx, close_rx));
        outbound_tx
            .send(Outbound::Frame {
                bytes: vec![0],
                principal_permit: registry.buffered_bytes(1000, 1).unwrap(),
                connection_permit: connection_buffer.reserve(1).unwrap(),
                guard: Some(OutputGuard {
                    deadline_at: tokio::time::Instant::now(),
                    control,
                    shutdown,
                    publication: None,
                }),
                delivery_deadline: tokio::time::Instant::now() + PARTIAL_FRAME_TIMEOUT,
                terminal_lease: Some(RequestLease {
                    active: Arc::new(Mutex::new(BTreeMap::new())),
                    request_id,
                    completion: Arc::new(Notify::new()),
                }),
            })
            .await
            .unwrap();
        let (completion, completed) = oneshot::channel();
        outbound_tx
            .send(Outbound::Barrier { completion })
            .await
            .unwrap();

        writer_task.await.unwrap();

        assert!(completed.await.is_err());
    }

    #[tokio::test]
    async fn guarded_enqueue_drops_stale_frame_without_closing_connection() {
        let registry = AdmissionRegistry::default();
        let connection_buffer = ConnectionBuffer::new(4096);
        let (outbound_tx, mut outbound_rx) = mpsc::channel(1);
        let (close_tx, close_rx) = watch::channel(false);
        outbound_tx
            .send(Outbound::Frame {
                bytes: vec![0],
                principal_permit: registry.buffered_bytes(1000, 1).unwrap(),
                connection_permit: connection_buffer.reserve(1).unwrap(),
                guard: None,
                delivery_deadline: tokio::time::Instant::now() + PARTIAL_FRAME_TIMEOUT,
                terminal_lease: None,
            })
            .await
            .unwrap();
        let (_control_tx, control) = watch::channel(ControlState {
            cancelled: false,
            worker_shutdown: false,
            acknowledged: 0,
            pending_acknowledgement: 0,
        });
        let (_shutdown_tx, shutdown) = watch::channel(false);
        let identifier =
            ConnectionIdentifier::parse("018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20").unwrap();

        queue_encoded(
            1000,
            &registry,
            &connection_buffer,
            (&outbound_tx, &close_tx),
            QueuedFrame {
                frame: ServerFrame::Response(Response {
                    server_connection_id: identifier,
                    request_id: RequestIdentifier::new(1).unwrap(),
                    result: ResponseResult::MockComplete,
                }),
                guard: Some(OutputGuard {
                    deadline_at: tokio::time::Instant::now() + Duration::from_millis(10),
                    control,
                    shutdown,
                    publication: None,
                }),
                delivery_deadline: tokio::time::Instant::now() + Duration::from_millis(10),
                terminal_lease: None,
            },
        )
        .await;

        assert!(!*close_rx.borrow());
        assert!(outbound_rx.try_recv().is_ok());
        assert!(outbound_rx.try_recv().is_err());
    }

    #[tokio::test]
    async fn absolute_request_deadline_bounds_stuck_dispatcher() {
        let identifier =
            ConnectionIdentifier::parse("018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20").unwrap();
        let context = DispatchContext {
            principal: PrincipalKey {
                target: WorkerTarget::User,
                uid: 1000,
            },
            peer: PeerCredentials {
                pid: 42,
                uid: 1000,
                gid: 100,
            },
            worker_epoch: identifier,
            connection_id: identifier,
            request_id: RequestIdentifier::new(1).unwrap(),
        };
        let operation = SemanticRequest::MockUnary { delay_ms: 0 };
        let (_control_sender, mut control) = watch::channel(ControlState {
            cancelled: false,
            worker_shutdown: false,
            acknowledged: 0,
            pending_acknowledgement: 0,
        });
        let (_shutdown_sender, mut shutdown) = watch::channel(false);

        let boundary = await_dispatch(
            &ReadyDispatcher,
            &context,
            &operation,
            tokio::time::Instant::now(),
            &mut control,
            &mut shutdown,
        )
        .await;
        assert_eq!(boundary, Err(DispatchInterruption::Deadline));

        let result = await_dispatch(
            &StuckDispatcher,
            &context,
            &operation,
            tokio::time::Instant::now() + Duration::from_millis(10),
            &mut control,
            &mut shutdown,
        )
        .await;

        assert_eq!(result, Err(DispatchInterruption::Deadline));
    }
}
