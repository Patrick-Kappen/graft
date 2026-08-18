#[cfg(feature = "worker-test-fixtures")]
use super::*;

#[cfg(feature = "worker-test-fixtures")]
pub(super) async fn run_mock_unary(
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
pub(super) async fn run_mock_stream(
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
pub(super) async fn await_output_barrier(context: &RequestContext) -> bool {
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
