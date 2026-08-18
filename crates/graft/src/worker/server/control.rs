use super::{
    watch, Arc, AtomicU64, BTreeMap, ConnectionIdentifier, Mutex, Notify, Ordering, WorkerErrorCode,
};

#[derive(Debug, Clone, Copy)]
pub(super) struct ControlState {
    #[cfg_attr(not(feature = "worker-test-fixtures"), allow(dead_code))]
    pub(super) cancelled: bool,
    #[cfg_attr(not(feature = "worker-test-fixtures"), allow(dead_code))]
    pub(super) worker_shutdown: bool,
    pub(super) acknowledged: u64,
    pub(super) pending_acknowledgement: u64,
}

pub(super) struct ActiveRequest {
    pub(super) control: watch::Sender<ControlState>,
    pub(super) produced: Arc<AtomicU64>,
    pub(super) delivered: Arc<AtomicU64>,
    pub(super) completion: Arc<Notify>,
    pub(super) stream: bool,
}

pub(super) type ActiveMap = Arc<Mutex<BTreeMap<crate::protocol::RequestIdentifier, ActiveRequest>>>;

pub(super) fn control_request(
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

pub(super) fn cancel_stream_interest(
    active: &ActiveMap,
    request_id: crate::protocol::RequestIdentifier,
) {
    if let Ok(active) = active.lock() {
        if let Some(request) = active.get(&request_id).filter(|request| request.stream) {
            request.control.send_modify(|state| state.cancelled = true);
        }
    }
}

pub(super) fn cancel_all(active: &ActiveMap, worker_shutdown: bool) {
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
