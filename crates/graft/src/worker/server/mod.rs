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
    encode_frame_exact, encoded_frame_len, negotiate_handshake, Capability, CapabilitySet,
    ClientHandshakeFrame, ConnectionIdentifier, EffectiveLimits, FrameDirection, HandshakeError,
    ManifestState, ProtocolError, ProtocolErrorCode, ProtocolVersionRange, SafeSummary,
    ServerHandshakeConfig, ServerHandshakeFrame, SoftwareVersion, WorkerContext,
};

use super::clock::WorkerEpoch;
use super::dispatcher::{
    DispatchContext, DispatchFailure, DispatchPlan, PeerCredentials, PrincipalKey,
    SemanticDispatcher,
};
#[cfg(feature = "worker-test-fixtures")]
use super::framing::PARTIAL_FRAME_TIMEOUT;
use super::framing::{read_frame, read_frame_with_timestamp, write_frame, AsyncFrameError};
use super::limits::{AdmissionPermit, AdmissionRegistry, ConnectionBuffer, ConnectionBufferPermit};
use super::protocol::{
    ClientFrame, ControlError, OperationPhase, Request, RequestError, Response, ResponseResult,
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

mod connection;
mod control;
mod fixture;
mod outbound;
mod request;

pub use connection::serve;
use control::{
    cancel_all, cancel_stream_interest, control_request, ActiveMap, ActiveRequest, ControlState,
};
#[cfg(feature = "worker-test-fixtures")]
use fixture::{run_mock_stream, run_mock_unary};
use outbound::{
    queue_control_error, queue_duplicate_error, queue_frame, writer_loop, Outbound, RequestLease,
};
#[cfg(feature = "worker-test-fixtures")]
use outbound::{DeliveryPublication, OutputGuard, StreamProgress};
use request::{frame_error, handshake_error, spawn_request, ConnectionEpoch, RequestContext};
#[cfg(feature = "worker-test-fixtures")]
use request::{send_error, worker_error};

#[cfg(all(test, feature = "worker-test-fixtures"))]
mod tests {
    use std::future::Future;
    use std::pin::Pin;

    use crate::protocol::{ManagerKind, RequestIdentifier, WorkerTarget};

    use super::super::limits::WorkerLimits;
    use super::connection::peer_is_authorized;
    use super::outbound::{queue_encoded, QueuedFrame};
    use super::request::{await_dispatch, DispatchInterruption};
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
