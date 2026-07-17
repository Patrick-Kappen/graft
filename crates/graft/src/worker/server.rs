//! Bounded socket-activated Unix worker server.

use std::collections::BTreeMap;
use std::io;
use std::os::fd::AsFd as _;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::io::{AsyncWriteExt as _, WriteHalf};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, watch, Semaphore};

use crate::protocol::{
    encode_frame, negotiate_handshake, CapabilitySet, ClientHandshakeFrame, ConnectionIdentifier,
    EffectiveLimits, FrameDirection, HandshakeError, ManifestState, ProtocolError,
    ProtocolErrorCode, ProtocolVersionRange, SafeSummary, ServerHandshakeConfig,
    ServerHandshakeFrame, SoftwareVersion, WorkerContext,
};

use super::clock::WorkerEpoch;
use super::dispatcher::{
    DispatchContext, DispatchPlan, PeerCredentials, PrincipalKey, SemanticDispatcher,
};
use super::framing::{read_frame, write_frame, AsyncFrameError, PARTIAL_FRAME_TIMEOUT};
use super::limits::{AdmissionPermit, AdmissionRegistry, ConnectionBuffer, ConnectionBufferPermit};
use super::protocol::{
    ClientFrame, Request, Response, ResponseResult, SemanticRequest, ServerFrame, WorkerError,
    WorkerErrorCode,
};
#[cfg(feature = "worker-test-fixtures")]
use super::protocol::{StreamEnd, StreamEndReason, StreamItem};

const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(5);
const OUTBOUND_QUEUE_ITEMS: usize = 64;

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
    /// Activated listener encountered a persistent accept failure.
    #[error("activated listener accept failed")]
    Accept(#[source] io::Error),
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
    let registry = AdmissionRegistry::default();
    let (shutdown_tx, mut shutdown_rx) = watch::channel(false);
    tokio::spawn(wait_for_shutdown(shutdown_tx));
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
                if config.context.target() == crate::protocol::WorkerTarget::User
                    && uid != config.context.effective_uid()
                {
                    continue;
                }
                let Some(connection_permit) = registry.connection(uid) else { continue; };
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

async fn wait_for_shutdown(sender: watch::Sender<bool>) {
    #[cfg(unix)]
    {
        let Ok(mut terminate) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
        else {
            return;
        };
        let Ok(mut interrupt) =
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())
        else {
            return;
        };
        tokio::select! {
            _ = terminate.recv() => {}
            _ = interrupt.recv() => {}
        }
        let _ = sender.send(true);
    }
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
        let Some(handshake_permit) = registry.handshake(uid) else {
            return;
        };
        let (mut reader, mut direct_writer) = tokio::io::split(stream);
        let hello = tokio::time::timeout(
            HANDSHAKE_TIMEOUT,
            read_frame::<_, ClientHandshakeFrame>(&mut reader),
        )
        .await;
        let client_hello = match hello {
            Ok(Ok(ClientHandshakeFrame::ClientHello(client_hello))) => client_hello,
            Ok(Err(error)) => {
                let frame = ServerHandshakeFrame::ProtocolError(frame_error(&error));
                let _ = write_frame(&mut direct_writer, &frame).await;
                return;
            }
            Err(_) => return,
        };
        drop(handshake_permit);

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
                let frame = ServerHandshakeFrame::ProtocolError(handshake_error(&error));
                let _ = write_frame(&mut direct_writer, &frame).await;
                return;
            }
        };
        if write_frame(
            &mut direct_writer,
            &ServerHandshakeFrame::ServerHello(server_hello.clone()),
        )
        .await
        .is_err()
        {
            return;
        }

        let (outbound_tx, outbound_rx) = mpsc::channel(OUTBOUND_QUEUE_ITEMS);
        let writer_registry = registry.clone();
        let writer = tokio::spawn(writer_loop(direct_writer, outbound_rx));
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
                frame = read_frame::<_, ClientFrame>(&mut reader) => {
                    let Ok(frame) = frame else { break; };
                    match frame {
                        ClientFrame::Request(request) => spawn_request(RequestContext {
                            request,
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
                            shutdown: shutdown.clone(),
                        }),
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
                                    uid, connection_id, control.request_id, code,
                                    &writer_registry, &connection_buffer, &outbound_tx,
                                ).await;
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
                                    uid, connection_id, control.request_id, code,
                                    &writer_registry, &connection_buffer, &outbound_tx,
                                ).await;
                            }
                        }
                    }
                }
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        shutting_down = true;
                        break;
                    }
                }
            }
        }
        cancel_all(&active, shutting_down);
        drop(outbound_tx);
        let _ = tokio::time::timeout(Duration::from_secs(2), writer).await;
    }
}

enum Outbound {
    Frame {
        bytes: Vec<u8>,
        _principal_permit: AdmissionPermit,
        _connection_permit: ConnectionBufferPermit,
    },
    Close,
}

async fn writer_loop(mut writer: WriteHalf<UnixStream>, mut receiver: mpsc::Receiver<Outbound>) {
    while let Some(outbound) = receiver.recv().await {
        let Outbound::Frame { bytes, .. } = outbound else {
            let _ = writer.shutdown().await;
            break;
        };
        if !matches!(
            tokio::time::timeout(PARTIAL_FRAME_TIMEOUT, writer.write_all(&bytes)).await,
            Ok(Ok(()))
        ) {
            break;
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct ControlState {
    #[cfg_attr(not(feature = "worker-test-fixtures"), allow(dead_code))]
    cancelled: bool,
    #[cfg_attr(not(feature = "worker-test-fixtures"), allow(dead_code))]
    worker_shutdown: bool,
    acknowledged: u64,
}

struct ActiveRequest {
    control: watch::Sender<ControlState>,
    emitted: Arc<AtomicU64>,
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
    let active = active
        .lock()
        .map_err(|_| WorkerErrorCode::RequestNotFound)?;
    let Some(request) = active.get(&request_id) else {
        return Err(WorkerErrorCode::RequestNotFound);
    };
    let current = *request.control.borrow();
    if cancel {
        let _ = request.control.send(ControlState {
            cancelled: true,
            ..current
        });
        return Ok(());
    }
    let Some(sequence) = sequence else {
        return Err(WorkerErrorCode::InvalidAcknowledgement);
    };
    if !request.stream
        || sequence < current.acknowledged
        || sequence > request.emitted.load(Ordering::Acquire)
    {
        let _ = request.control.send(ControlState {
            cancelled: true,
            ..current
        });
        return Err(WorkerErrorCode::InvalidAcknowledgement);
    }
    let _ = request.control.send(ControlState {
        acknowledged: sequence,
        ..current
    });
    Ok(())
}

fn cancel_all(active: &ActiveMap, worker_shutdown: bool) {
    if let Ok(active) = active.lock() {
        for request in active.values() {
            let current = *request.control.borrow();
            let _ = request.control.send(ControlState {
                cancelled: !worker_shutdown,
                worker_shutdown,
                ..current
            });
        }
    }
}

struct RequestContext {
    request: Request,
    expected_connection: ConnectionIdentifier,
    peer: PeerCredentials,
    principal: PrincipalKey,
    limits: EffectiveLimits,
    epoch: Arc<WorkerEpoch>,
    dispatcher: Arc<dyn SemanticDispatcher>,
    registry: AdmissionRegistry,
    request_slots: Arc<Semaphore>,
    stream_slots: Arc<Semaphore>,
    active: ActiveMap,
    connection_buffer: ConnectionBuffer,
    outbound: mpsc::Sender<Outbound>,
    shutdown: watch::Receiver<bool>,
}

#[cfg(feature = "worker-test-fixtures")]
const fn is_stream_operation(operation: &SemanticRequest) -> bool {
    matches!(operation, SemanticRequest::MockStream { .. })
}

#[cfg(not(feature = "worker-test-fixtures"))]
const fn is_stream_operation(_operation: &SemanticRequest) -> bool {
    false
}

fn spawn_request(context: RequestContext) {
    if context.request.server_connection_id != context.expected_connection {
        send_error(
            &context,
            WorkerErrorCode::ConnectionMismatch,
            "connection identifier mismatch",
        );
        return;
    }
    let deadline_ms = context
        .request
        .deadline_ms
        .unwrap_or(context.limits.unary_deadline_ms());
    if deadline_ms == 0 || deadline_ms > context.limits.unary_deadline_ms() {
        send_error(
            &context,
            WorkerErrorCode::Deadline,
            "request deadline is invalid",
        );
        return;
    }
    let deadline_at = tokio::time::Instant::now() + Duration::from_millis(deadline_ms);
    let Ok(connection_permit) = context.request_slots.clone().try_acquire_owned() else {
        send_error(
            &context,
            WorkerErrorCode::Overloaded,
            "connection request limit reached",
        );
        return;
    };
    let Some(principal_permit) = context.registry.request(context.peer.uid) else {
        send_error(
            &context,
            WorkerErrorCode::Overloaded,
            "worker request limit reached",
        );
        return;
    };
    let (control_tx, control_rx) = watch::channel(ControlState {
        cancelled: false,
        worker_shutdown: false,
        acknowledged: 0,
    });
    let emitted = Arc::new(AtomicU64::new(0));
    let stream = is_stream_operation(&context.request.operation);
    {
        let Ok(mut active) = context.active.lock() else {
            return;
        };
        if active.contains_key(&context.request.request_id) {
            drop(active);
            send_error(
                &context,
                WorkerErrorCode::RequestConflict,
                "request identifier is already active",
            );
            return;
        }
        active.insert(
            context.request.request_id,
            ActiveRequest {
                control: control_tx,
                emitted: emitted.clone(),
                stream,
            },
        );
    }
    tokio::spawn(async move {
        let request_id = context.request.request_id;
        let active_requests = context.active.clone();
        run_request(context, control_rx, emitted, deadline_at).await;
        if let Ok(mut active) = active_requests.lock() {
            active.remove(&request_id);
        }
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
    let dispatch = dispatcher.dispatch(context, operation);
    tokio::pin!(dispatch);
    loop {
        tokio::select! {
            plan = &mut dispatch => return Ok(plan),
            () = tokio::time::sleep_until(deadline_at) => {
                return Err(DispatchInterruption::Deadline);
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
            changed = shutdown.changed() => {
                let _ = changed;
                return Err(DispatchInterruption::WorkerShutdown);
            }
        }
    }
}

async fn run_request(
    context: RequestContext,
    #[allow(unused_mut)] mut control: watch::Receiver<ControlState>,
    emitted: Arc<AtomicU64>,
    deadline_at: tokio::time::Instant,
) {
    #[cfg(not(feature = "worker-test-fixtures"))]
    let _ = (&control, &emitted, deadline_at);
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
                        WorkerErrorCode::Unsupported,
                        "semantic operation is unavailable",
                    ),
                }),
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
                emitted,
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
    let result = tokio::select! {
        () = tokio::time::sleep(Duration::from_millis(delay_ms)) => ResponseResult::MockComplete,
        () = tokio::time::sleep_until(deadline_at) => worker_error(WorkerErrorCode::Deadline, "request deadline elapsed"),
        changed = control.changed() => {
            let _ = changed;
            let state = *control.borrow();
            if state.worker_shutdown {
                worker_error(WorkerErrorCode::WorkerShutdown, "worker shutdown")
            } else {
                worker_error(WorkerErrorCode::Cancelled, "request cancelled")
            }
        }
        changed = request_shutdown.changed() => {
            let _ = changed;
            worker_error(WorkerErrorCode::WorkerShutdown, "worker shutdown")
        }
    };
    queue_frame(
        context,
        ServerFrame::Response(Response {
            server_connection_id: context.expected_connection,
            request_id: context.request.request_id,
            result,
        }),
    )
    .await;
}

#[cfg(feature = "worker-test-fixtures")]
#[allow(clippy::too_many_lines)]
async fn run_mock_stream(
    context: RequestContext,
    control: &mut watch::Receiver<ControlState>,
    emitted: Arc<AtomicU64>,
    deadline_at: tokio::time::Instant,
    items: u32,
    interval_ms: u64,
) {
    let Ok(connection_stream) = context.stream_slots.clone().try_acquire_owned() else {
        send_error(
            &context,
            WorkerErrorCode::Overloaded,
            "connection stream limit reached",
        );
        return;
    };
    let Some(principal_stream) = context.registry.stream(context.peer.uid) else {
        send_error(
            &context,
            WorkerErrorCode::Overloaded,
            "worker stream limit reached",
        );
        return;
    };
    let mut stream_shutdown = context.shutdown.clone();
    let mut reason = StreamEndReason::Completed;
    let mut sequence = 0_u64;
    'items: for value in 1..=items {
        let stalled_at = tokio::time::Instant::now() + Duration::from_secs(1);
        while sequence.saturating_sub(control.borrow().acknowledged)
            >= u64::from(context.limits.unacknowledged_stream_items())
        {
            tokio::select! {
                () = tokio::time::sleep_until(stalled_at) => {
                    reason = StreamEndReason::SlowConsumer;
                    break 'items;
                }
                () = tokio::time::sleep_until(deadline_at) => {
                    reason = StreamEndReason::Deadline;
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
        let item_at = tokio::time::Instant::now() + Duration::from_millis(interval_ms);
        loop {
            tokio::select! {
                () = tokio::time::sleep_until(item_at) => break,
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
                () = tokio::time::sleep_until(deadline_at) => {
                    reason = StreamEndReason::Deadline;
                    break 'items;
                }
            }
        }
        sequence = sequence.saturating_add(1);
        emitted.store(sequence, Ordering::Release);
        queue_frame(
            &context,
            ServerFrame::StreamItem(StreamItem {
                server_connection_id: context.expected_connection,
                request_id: context.request.request_id,
                worker_epoch: context.epoch.identifier(),
                sequence,
                mock_value: value,
            }),
        )
        .await;
    }
    queue_frame(
        &context,
        ServerFrame::StreamEnd(StreamEnd {
            server_connection_id: context.expected_connection,
            request_id: context.request.request_id,
            worker_epoch: context.epoch.identifier(),
            reason,
            final_sequence: sequence,
        }),
    )
    .await;
    drop(principal_stream);
    drop(connection_stream);
}

async fn queue_request_error(
    context: &RequestContext,
    code: WorkerErrorCode,
    summary: &'static str,
) {
    queue_frame(
        context,
        ServerFrame::Response(Response {
            server_connection_id: context.expected_connection,
            request_id: context.request.request_id,
            result: worker_error(code, summary),
        }),
    )
    .await;
}

fn send_error(context: &RequestContext, code: WorkerErrorCode, summary: &'static str) {
    let frame = ServerFrame::Response(Response {
        server_connection_id: context.expected_connection,
        request_id: context.request.request_id,
        result: worker_error(code, summary),
    });
    let context = context.clone_for_send();
    tokio::spawn(async move {
        queue_frame(&context, frame).await;
    });
}

impl RequestContext {
    fn clone_for_send(&self) -> Self {
        Self {
            request: self.request.clone(),
            expected_connection: self.expected_connection,
            peer: self.peer,
            principal: self.principal,
            limits: self.limits,
            epoch: self.epoch.clone(),
            dispatcher: self.dispatcher.clone(),
            registry: self.registry.clone(),
            request_slots: self.request_slots.clone(),
            stream_slots: self.stream_slots.clone(),
            active: self.active.clone(),
            connection_buffer: self.connection_buffer.clone(),
            outbound: self.outbound.clone(),
            shutdown: self.shutdown.clone(),
        }
    }
}

async fn queue_frame(context: &RequestContext, frame: ServerFrame) {
    queue_encoded(
        context.peer.uid,
        &context.registry,
        &context.connection_buffer,
        &context.outbound,
        frame,
    )
    .await;
}

async fn queue_control_error(
    uid: u32,
    connection_id: ConnectionIdentifier,
    request_id: crate::protocol::RequestIdentifier,
    code: WorkerErrorCode,
    registry: &AdmissionRegistry,
    connection_buffer: &ConnectionBuffer,
    outbound: &mpsc::Sender<Outbound>,
) {
    queue_encoded(
        uid,
        registry,
        connection_buffer,
        outbound,
        ServerFrame::Response(Response {
            server_connection_id: connection_id,
            request_id,
            result: worker_error(code, "request control frame is invalid"),
        }),
    )
    .await;
}

async fn queue_encoded(
    uid: u32,
    registry: &AdmissionRegistry,
    connection_buffer: &ConnectionBuffer,
    outbound: &mpsc::Sender<Outbound>,
    frame: ServerFrame,
) {
    let Ok(bytes) = encode_frame(&frame, FrameDirection::ServerToClient) else {
        let _ = outbound.send(Outbound::Close).await;
        return;
    };
    let Some(connection_permit) = connection_buffer.reserve(bytes.len()) else {
        let _ = outbound.send(Outbound::Close).await;
        return;
    };
    let Some(principal_permit) = registry.buffered_bytes(uid, bytes.len()) else {
        let _ = outbound.send(Outbound::Close).await;
        return;
    };
    let _ = outbound
        .send(Outbound::Frame {
            bytes,
            _principal_permit: principal_permit,
            _connection_permit: connection_permit,
        })
        .await;
}

fn worker_error(code: WorkerErrorCode, summary: &'static str) -> ResponseResult {
    let summary = SafeSummary::parse(summary).expect("static worker summary is valid");
    ResponseResult::Error(WorkerError { code, summary })
}

fn frame_error(error: &AsyncFrameError) -> ProtocolError {
    let code = match error {
        AsyncFrameError::Length | AsyncFrameError::Encode(_) => ProtocolErrorCode::LimitExceeded,
        AsyncFrameError::Io(_)
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

    use crate::protocol::{RequestIdentifier, WorkerTarget};

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
        });
        let (_shutdown_sender, mut shutdown) = watch::channel(false);

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
