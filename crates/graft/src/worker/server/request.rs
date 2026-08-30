use super::*;

#[derive(Debug, Clone, Copy)]
pub(super) struct ConnectionEpoch {
    pub(super) connection_id: ConnectionIdentifier,
    pub(super) worker_epoch: ConnectionIdentifier,
}

pub(super) struct RequestContext {
    pub(super) request: Request,
    pub(super) received_at: tokio::time::Instant,
    pub(super) expected_connection: ConnectionIdentifier,
    pub(super) peer: PeerCredentials,
    pub(super) principal: PrincipalKey,
    pub(super) limits: EffectiveLimits,
    pub(super) capabilities: CapabilitySet,
    pub(super) epoch: Arc<WorkerEpoch>,
    pub(super) dispatcher: Arc<dyn SemanticDispatcher>,
    pub(super) registry: AdmissionRegistry,
    pub(super) request_slots: Arc<Semaphore>,
    #[cfg_attr(not(feature = "worker-test-fixtures"), allow(dead_code))]
    pub(super) stream_slots: Arc<Semaphore>,
    pub(super) active: ActiveMap,
    pub(super) connection_buffer: ConnectionBuffer,
    pub(super) outbound: mpsc::Sender<Outbound>,
    pub(super) connection_close: watch::Sender<bool>,
    pub(super) completion: Arc<Notify>,
    pub(super) terminal_lease: Arc<Mutex<Option<RequestLease>>>,
    pub(super) request_deadline: Arc<Mutex<Option<tokio::time::Instant>>>,
    pub(super) shutdown: watch::Receiver<bool>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum OperationDeadlineClass {
    Unary,
    #[cfg_attr(not(feature = "worker-test-fixtures"), allow(dead_code))]
    Lifecycle,
}

const fn operation_deadline_class(operation: &SemanticRequest) -> OperationDeadlineClass {
    match operation {
        SemanticRequest::ListStatus(_)
        | SemanticRequest::GetStatus { .. }
        | SemanticRequest::Inspect { .. }
        | SemanticRequest::QueryLifecycle(_)
        | SemanticRequest::Reserved => OperationDeadlineClass::Unary,
        SemanticRequest::Lifecycle(_) => OperationDeadlineClass::Lifecycle,
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
pub(super) async fn spawn_request(context: RequestContext) {
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
        queue_duplicate_error(&context).await;
        return;
    }
    let required_capability = match &context.request.operation {
        SemanticRequest::ListStatus(_) | SemanticRequest::GetStatus { .. } => {
            Some(Capability::Observe)
        }
        SemanticRequest::Inspect { .. } => Some(Capability::Inspect),
        SemanticRequest::Lifecycle(_) | SemanticRequest::QueryLifecycle(_) => {
            Some(Capability::Lifecycle)
        }
        SemanticRequest::Reserved => None,
        #[cfg(feature = "worker-test-fixtures")]
        SemanticRequest::MockUnary { .. }
        | SemanticRequest::MockLifecycle { .. }
        | SemanticRequest::MockStream { .. } => None,
    };
    if required_capability.is_some_and(|capability| !context.capabilities.contains(capability)) {
        if let Ok(mut active) = context.active.lock() {
            active.remove(&context.request.request_id);
        }
        send_error(
            &context,
            WorkerErrorCode::Unsupported,
            "operation capability was not negotiated",
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
pub(super) enum DispatchInterruption {
    Deadline,
    Cancelled,
    WorkerShutdown,
}

pub(super) async fn await_dispatch(
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

pub(super) async fn run_request(
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
        DispatchPlan::Unary(result) => queue_unary_dispatch(&context, *result).await,
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

pub(super) async fn queue_unary_dispatch(
    context: &RequestContext,
    result: Result<super::super::protocol::ReadOnlyResponse, DispatchFailure>,
) {
    let result = match result {
        Ok(response) => ResponseResult::ReadOnly(Box::new(response)),
        Err(error) => dispatch_error(context.epoch.identifier(), error),
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

pub(super) async fn queue_request_error(
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

pub(super) async fn send_error(
    context: &RequestContext,
    code: WorkerErrorCode,
    summary: &'static str,
) {
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

pub(super) fn dispatch_error(
    worker_epoch: ConnectionIdentifier,
    error: DispatchFailure,
) -> ResponseResult {
    let retry = match error.code {
        WorkerErrorCode::StaleManifest
        | WorkerErrorCode::WorkloadNotFound
        | WorkerErrorCode::PageCursorExpired => RetryClassification::AfterStateRefresh,
        WorkerErrorCode::Unauthorized => RetryClassification::AfterAuthorization,
        WorkerErrorCode::ManifestUnavailable | WorkerErrorCode::Unavailable => {
            RetryClassification::AfterBackendRecovery
        }
        WorkerErrorCode::Overloaded => RetryClassification::SameRequestWithBackoff,
        _ => RetryClassification::Never,
    };
    ResponseResult::Error(WorkerError {
        code: error.code,
        summary: error.summary,
        retry,
        phase: OperationPhase::Execution,
        worker_epoch,
    })
}

pub(super) fn worker_error(
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
        WorkerErrorCode::RequestConflict
        | WorkerErrorCode::RequestNotFound
        | WorkerErrorCode::StaleManifest
        | WorkerErrorCode::WorkloadNotFound
        | WorkerErrorCode::PageCursorExpired => RetryClassification::AfterStateRefresh,
        WorkerErrorCode::Unauthorized => RetryClassification::AfterAuthorization,
        WorkerErrorCode::ManifestUnavailable | WorkerErrorCode::Unavailable => {
            RetryClassification::AfterBackendRecovery
        }
        WorkerErrorCode::ConnectionMismatch
        | WorkerErrorCode::Deadline
        | WorkerErrorCode::Cancelled
        | WorkerErrorCode::InvalidAcknowledgement
        | WorkerErrorCode::Unsupported
        | WorkerErrorCode::InvalidRequest => RetryClassification::Never,
    };
    ResponseResult::Error(WorkerError {
        code,
        summary,
        retry,
        phase,
        worker_epoch,
    })
}

pub(super) fn frame_error(error: &AsyncFrameError) -> ProtocolError {
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

pub(super) fn handshake_error(error: &HandshakeError) -> ProtocolError {
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
