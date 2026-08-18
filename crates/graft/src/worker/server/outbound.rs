use super::*;

#[cfg(feature = "worker-test-fixtures")]
#[derive(Clone)]
pub(super) struct StreamProgress {
    pub(super) produced: Arc<AtomicU64>,
    pub(super) delivered: Arc<AtomicU64>,
    pub(super) control: watch::Sender<ControlState>,
}

#[derive(Clone)]
pub(super) struct DeliveryPublication {
    pub(super) delivered: Arc<AtomicU64>,
    pub(super) control: watch::Sender<ControlState>,
    pub(super) sequence: u64,
}

#[derive(Clone)]
pub(super) struct OutputGuard {
    pub(super) deadline_at: tokio::time::Instant,
    pub(super) control: watch::Receiver<ControlState>,
    pub(super) shutdown: watch::Receiver<bool>,
    pub(super) publication: Option<DeliveryPublication>,
}

pub(super) struct RequestLease {
    pub(super) active: ActiveMap,
    pub(super) request_id: crate::protocol::RequestIdentifier,
    pub(super) completion: Arc<Notify>,
}

impl Drop for RequestLease {
    fn drop(&mut self) {
        if let Ok(mut active) = self.active.lock() {
            active.remove(&self.request_id);
        }
        self.completion.notify_one();
    }
}

pub(super) enum WriteOutcome {
    Written,
    Skipped,
    Failed,
}

pub(super) enum Outbound {
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
pub(super) async fn writer_loop(
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

pub(super) async fn queue_frame(
    context: &RequestContext,
    frame: ServerFrame,
    guard: Option<OutputGuard>,
) {
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

pub(super) async fn queue_duplicate_error(context: &RequestContext) {
    if !context.registry.rejection(context.peer.uid) {
        let _ = context.connection_close.send(true);
        return;
    }
    queue_encoded(
        context.peer.uid,
        &context.registry,
        &context.connection_buffer,
        (&context.outbound, &context.connection_close),
        QueuedFrame {
            frame: ServerFrame::RequestError(RequestError {
                server_connection_id: context.expected_connection,
                request_id: context.request.request_id,
                error: WorkerError {
                    code: WorkerErrorCode::RequestConflict,
                    summary: SafeSummary::parse("request identifier is already active")
                        .expect("fixed conflict summary is valid"),
                    retry: RetryClassification::AfterStateRefresh,
                    phase: OperationPhase::Admission,
                    worker_epoch: context.epoch.identifier(),
                },
            }),
            guard: None,
            delivery_deadline: tokio::time::Instant::now() + TERMINAL_DELIVERY_TIMEOUT,
            terminal_lease: None,
        },
    )
    .await;
}

pub(super) async fn queue_control_error(
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

pub(super) struct QueuedFrame {
    pub(super) frame: ServerFrame,
    pub(super) guard: Option<OutputGuard>,
    pub(super) delivery_deadline: tokio::time::Instant,
    pub(super) terminal_lease: Option<RequestLease>,
}

pub(super) async fn queue_encoded(
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
