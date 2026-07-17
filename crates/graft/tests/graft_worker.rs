use std::io::{Read as _, Write as _};
use std::os::fd::AsRawFd as _;
use std::os::unix::net::{UnixListener, UnixStream};
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[cfg(feature = "worker-test-fixtures")]
use graft::protocol::RequestIdentifier;
use graft::protocol::{
    decode_frame, encode_frame, CapabilitySet, ClientComponent, ClientHandshakeFrame, ClientHello,
    ConnectionIdentifier, EffectiveLimits, FrameDirection, ProtocolVersionRange,
    ServerHandshakeFrame, SoftwareVersion,
};
#[cfg(feature = "worker-test-fixtures")]
use graft::worker::protocol::{
    Cancel, ClientFrame, OperationPhase, Request, ResponseResult, RetryClassification,
    SemanticRequest, ServerFrame, StreamAck, StreamEndReason,
};
use tempfile::TempDir;

#[cfg(feature = "worker-test-fixtures")]
const WORKER_BINARY: &str = env!("CARGO_BIN_EXE_graft-worker-fixture");
#[cfg(not(feature = "worker-test-fixtures"))]
const WORKER_BINARY: &str = env!("CARGO_BIN_EXE_graft-worker");

struct WorkerProcess {
    child: Child,
    socket_path: std::path::PathBuf,
    _temporary: TempDir,
}

impl Drop for WorkerProcess {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn spawn_worker() -> WorkerProcess {
    spawn_worker_binary(WORKER_BINARY)
}

fn spawn_worker_binary(binary: &str) -> WorkerProcess {
    let temporary = TempDir::new().unwrap();
    let socket_path = temporary.path().join("worker.sock");
    let listener = UnixListener::bind(&socket_path).unwrap();
    let listener_fd = listener.as_raw_fd();
    let config_home = temporary.path().join("config");
    std::fs::create_dir(&config_home).unwrap();
    let uid = rustix::process::geteuid().as_raw();
    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg("export LISTEN_PID=$$; exec \"$@\"")
        .arg("graft-worker-wrapper")
        .arg(binary)
        .args([
            "--target",
            "user",
            "--effective-uid",
            &uid.to_string(),
            "--manager",
            "user",
            "--config-home",
            config_home.to_str().unwrap(),
            "--producer-name",
            "graft",
            "--producer-version",
            env!("CARGO_PKG_VERSION"),
            "--producer-build-id",
            "worker-test",
        ])
        .env("LISTEN_FDS", "1")
        .env("LISTEN_FDNAMES", "graft-worker")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    // SAFETY: the child-only pre-exec closure duplicates the live parent
    // listener onto systemd's descriptor 3. No allocation or non-async-signal-
    // safe library operation is performed after fork.
    unsafe {
        command.pre_exec(move || {
            if libc::dup2(listener_fd, 3) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            if libc::fcntl(3, libc::F_SETFD, 0) == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let child = command.spawn().unwrap();
    drop(listener);
    WorkerProcess {
        child,
        socket_path,
        _temporary: temporary,
    }
}

fn connect(path: &Path) -> UnixStream {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        match UnixStream::connect(path) {
            Ok(stream) => {
                stream
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                return stream;
            }
            Err(error) if Instant::now() < deadline => {
                let _ = error;
                thread::sleep(Duration::from_millis(10));
            }
            Err(error) => panic!("worker did not accept connections: {error}"),
        }
    }
}

fn handshake(stream: &mut UnixStream) -> graft::protocol::ServerHello {
    handshake_with_limits(stream, EffectiveLimits::protocol_maxima())
}

fn handshake_with_limits(
    stream: &mut UnixStream,
    requested_limits: EffectiveLimits,
) -> graft::protocol::ServerHello {
    let hello = ClientHandshakeFrame::ClientHello(ClientHello {
        protocol: ProtocolVersionRange::new(1, 0, 0).unwrap(),
        component: ClientComponent::Cli,
        software_version: SoftwareVersion::parse("test-client").unwrap(),
        requested_capabilities: CapabilitySet::new([]).unwrap(),
        requested_limits,
        client_connection_id: ConnectionIdentifier::parse("018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20")
            .unwrap(),
    });
    stream
        .write_all(&encode_frame(&hello, FrameDirection::ClientToServer).unwrap())
        .unwrap();
    match read_server_frame::<ServerHandshakeFrame>(stream) {
        ServerHandshakeFrame::ServerHello(hello) => hello,
        ServerHandshakeFrame::ProtocolError(error) => panic!("handshake failed: {:?}", error.code),
    }
}

fn read_server_frame<T: serde::de::DeserializeOwned>(stream: &mut UnixStream) -> T {
    let mut prefix = [0_u8; 4];
    stream.read_exact(&mut prefix).unwrap();
    let length = usize::try_from(u32::from_be_bytes(prefix)).unwrap();
    let mut frame = Vec::with_capacity(length + 4);
    frame.extend_from_slice(&prefix);
    frame.resize(length + 4, 0);
    stream.read_exact(&mut frame[4..]).unwrap();
    decode_frame(&frame, FrameDirection::ServerToClient).unwrap()
}

#[test]
fn malformed_and_oversized_initial_frames_receive_typed_protocol_errors() {
    for (frame, expected_code) in [
        (
            {
                let payload = b"{}";
                let mut frame = Vec::from(u32::try_from(payload.len()).unwrap().to_be_bytes());
                frame.extend_from_slice(payload);
                frame
            },
            graft::protocol::ProtocolErrorCode::Malformed,
        ),
        (
            0_u32.to_be_bytes().to_vec(),
            graft::protocol::ProtocolErrorCode::Malformed,
        ),
        (
            u32::try_from(graft::protocol::MAX_INBOUND_FRAME_BYTES + 1)
                .unwrap()
                .to_be_bytes()
                .to_vec(),
            graft::protocol::ProtocolErrorCode::LimitExceeded,
        ),
    ] {
        let worker = spawn_worker();
        let mut stream = connect(&worker.socket_path);
        stream.write_all(&frame).unwrap();

        let response = read_server_frame::<ServerHandshakeFrame>(&mut stream);

        assert!(matches!(
            response,
            ServerHandshakeFrame::ProtocolError(error) if error.code == expected_code
        ));
    }
}

#[test]
fn real_worker_process_handshake_uses_inherited_unix_socket_and_fresh_epoch() {
    let mut first = spawn_worker();
    let mut first_stream = connect(&first.socket_path);
    let first_hello = handshake(&mut first_stream);
    first.child.kill().unwrap();
    first.child.wait().unwrap();

    let second = spawn_worker();
    let mut second_stream = connect(&second.socket_path);
    let second_hello = handshake(&mut second_stream);

    assert_ne!(first_hello.worker_epoch, second_hello.worker_epoch);
    assert_ne!(
        first_hello.server_connection_id,
        second_hello.server_connection_id
    );
}

#[derive(Clone, Copy)]
enum ListenPid<'a> {
    Child,
    Missing,
    Value(&'a str),
}

fn invalid_activation_status(
    fds: &str,
    name: &str,
    inherited_fd: Option<i32>,
    listen_pid: ListenPid<'_>,
) -> bool {
    let mut command = Command::new("bash");
    command
        .arg("-c")
        .arg(match listen_pid {
            ListenPid::Child => "export LISTEN_PID=$$; exec \"$@\"",
            ListenPid::Missing | ListenPid::Value(_) => "exec \"$@\"",
        })
        .arg("graft-worker-wrapper")
        .arg(WORKER_BINARY)
        .args([
            "--target",
            "user",
            "--effective-uid",
            "1000",
            "--manager",
            "user",
            "--config-home",
            "/tmp",
            "--producer-name",
            "graft",
            "--producer-version",
            env!("CARGO_PKG_VERSION"),
            "--producer-build-id",
            "worker-test",
        ])
        .env("LISTEN_FDS", fds)
        .env("LISTEN_FDNAMES", name)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    match listen_pid {
        ListenPid::Child => {}
        ListenPid::Missing => {
            command.env_remove("LISTEN_PID");
        }
        ListenPid::Value(value) => {
            command.env("LISTEN_PID", value);
        }
    }
    if let Some(source_fd) = inherited_fd {
        // SAFETY: the child-only closure duplicates one live test descriptor to
        // fd 3 and clears close-on-exec using async-signal-safe syscalls.
        unsafe {
            command.pre_exec(move || {
                if libc::dup2(source_fd, 3) == -1 || libc::fcntl(3, libc::F_SETFD, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
    }
    command.status().unwrap().success()
}

#[test]
fn worker_rejects_invalid_activation_cardinality_name_and_type() {
    assert!(!invalid_activation_status(
        "0",
        "graft-worker",
        None,
        ListenPid::Child
    ));
    assert!(!invalid_activation_status(
        "2",
        "graft-worker",
        None,
        ListenPid::Child
    ));
    assert!(!invalid_activation_status(
        "1",
        "wrong-name",
        None,
        ListenPid::Child
    ));
    assert!(!invalid_activation_status(
        "1",
        "graft-worker",
        None,
        ListenPid::Child
    ));
    let regular_file = std::fs::File::open("/dev/null").unwrap();
    assert!(!invalid_activation_status(
        "1",
        "graft-worker",
        Some(regular_file.as_raw_fd()),
        ListenPid::Child
    ));
    for listen_pid in [
        ListenPid::Missing,
        ListenPid::Value("invalid"),
        ListenPid::Value("1"),
    ] {
        assert!(!invalid_activation_status(
            "1",
            "graft-worker",
            Some(regular_file.as_raw_fd()),
            listen_pid,
        ));
    }
}

#[cfg(feature = "worker-test-fixtures")]
fn send_client_frame(stream: &mut UnixStream, frame: &ClientFrame) {
    stream
        .write_all(&encode_frame(frame, FrameDirection::ClientToServer).unwrap())
        .unwrap();
}

#[cfg(feature = "worker-test-fixtures")]
#[test]
fn production_worker_remains_unsupported_with_all_features() {
    let worker = spawn_worker_binary(env!("CARGO_BIN_EXE_graft-worker"));
    let mut stream = connect(&worker.socket_path);
    let hello = handshake(&mut stream);
    send_client_frame(
        &mut stream,
        &ClientFrame::Request(Request {
            server_connection_id: hello.server_connection_id,
            request_id: RequestIdentifier::new(1).unwrap(),
            deadline_ms: Some(1_000),
            operation: SemanticRequest::MockUnary { delay_ms: 0 },
        }),
    );

    let response = read_server_frame::<ServerFrame>(&mut stream);

    assert!(matches!(
        response,
        ServerFrame::Response(response)
            if matches!(&response.result, ResponseResult::Error(error)
                if error.code == graft::worker::protocol::WorkerErrorCode::Unsupported)
    ));
}

#[cfg(feature = "worker-test-fixtures")]
#[test]
fn real_worker_process_dispatches_typed_mock_unary_request() {
    let worker = spawn_worker();
    let mut stream = connect(&worker.socket_path);
    let hello = handshake(&mut stream);
    let request_id = RequestIdentifier::new(1).unwrap();
    let request = ClientFrame::Request(Request {
        server_connection_id: hello.server_connection_id,
        request_id,
        deadline_ms: Some(1_000),
        operation: SemanticRequest::MockUnary { delay_ms: 0 },
    });
    send_client_frame(&mut stream, &request);

    let response = read_server_frame::<ServerFrame>(&mut stream);

    assert!(matches!(
        response,
        ServerFrame::Response(response)
            if response.request_id == request_id
                && response.result == ResponseResult::MockComplete
    ));

    send_client_frame(&mut stream, &request);
    assert!(matches!(
        read_server_frame::<ServerFrame>(&mut stream),
        ServerFrame::Response(response)
            if response.request_id == request_id
                && response.result == ResponseResult::MockComplete
    ));
}

#[cfg(feature = "worker-test-fixtures")]
#[test]
fn real_worker_stream_sequences_acknowledgements_and_cancellation() {
    let worker = spawn_worker();
    let mut stream = connect(&worker.socket_path);
    let maxima = EffectiveLimits::protocol_maxima();
    let limits = EffectiveLimits::new(
        maxima.concurrent_requests(),
        maxima.active_streams(),
        maxima.buffered_response_bytes(),
        1,
        maxima.workloads_per_page(),
        maxima.log_records_per_page(),
        maxima.encoded_log_message_bytes(),
        maxima.unary_deadline_ms(),
        maxima.lifecycle_deadline_ms(),
    )
    .unwrap();
    let hello = handshake_with_limits(&mut stream, limits);
    let request_id = RequestIdentifier::new(2).unwrap();
    send_client_frame(
        &mut stream,
        &ClientFrame::Request(Request {
            server_connection_id: hello.server_connection_id,
            request_id,
            deadline_ms: Some(5_000),
            operation: SemanticRequest::MockStream {
                items: 3,
                interval_ms: 50,
            },
        }),
    );
    let first = read_server_frame::<ServerFrame>(&mut stream);
    assert!(matches!(first, ServerFrame::StreamItem(item) if item.sequence == 1));
    send_client_frame(
        &mut stream,
        &ClientFrame::StreamAck(StreamAck {
            server_connection_id: hello.server_connection_id,
            request_id,
            sequence: 0,
        }),
    );
    send_client_frame(
        &mut stream,
        &ClientFrame::Cancel(Cancel {
            server_connection_id: hello.server_connection_id,
            request_id,
        }),
    );

    let terminal = read_server_frame::<ServerFrame>(&mut stream);

    assert!(matches!(
        terminal,
        ServerFrame::StreamEnd(end)
            if end.reason == StreamEndReason::Cancelled
                && end.worker_epoch == hello.worker_epoch
    ));
}

#[cfg(feature = "worker-test-fixtures")]
#[test]
fn real_worker_unary_deadline_and_cancellation_are_typed() {
    let worker = spawn_worker();
    let mut stream = connect(&worker.socket_path);
    let hello = handshake(&mut stream);
    let deadline_id = RequestIdentifier::new(3).unwrap();
    send_client_frame(
        &mut stream,
        &ClientFrame::Request(Request {
            server_connection_id: hello.server_connection_id,
            request_id: deadline_id,
            deadline_ms: Some(10),
            operation: SemanticRequest::MockUnary { delay_ms: 1_000 },
        }),
    );
    let deadline = read_server_frame::<ServerFrame>(&mut stream);
    assert!(matches!(
        deadline,
        ServerFrame::Response(response)
            if matches!(&response.result, ResponseResult::Error(error)
                if error.code == graft::worker::protocol::WorkerErrorCode::Deadline
                    && error.retry == RetryClassification::Never
                    && error.phase == OperationPhase::Execution
                    && error.worker_epoch == hello.worker_epoch)
    ));

    let cancel_id = RequestIdentifier::new(4).unwrap();
    send_client_frame(
        &mut stream,
        &ClientFrame::Request(Request {
            server_connection_id: hello.server_connection_id,
            request_id: cancel_id,
            deadline_ms: Some(5_000),
            operation: SemanticRequest::MockUnary { delay_ms: 1_000 },
        }),
    );
    send_client_frame(
        &mut stream,
        &ClientFrame::Cancel(Cancel {
            server_connection_id: hello.server_connection_id,
            request_id: cancel_id,
        }),
    );
    let cancelled = read_server_frame::<ServerFrame>(&mut stream);
    assert!(matches!(
        cancelled,
        ServerFrame::Response(response)
            if matches!(&response.result, ResponseResult::Error(error)
                if error.code == graft::worker::protocol::WorkerErrorCode::Cancelled)
    ));
}

#[cfg(feature = "worker-test-fixtures")]
#[test]
fn negotiated_connection_request_limit_rejects_new_work_without_eviction() {
    let worker = spawn_worker();
    let mut stream = connect(&worker.socket_path);
    let maxima = EffectiveLimits::protocol_maxima();
    let limits = EffectiveLimits::new(
        1,
        maxima.active_streams(),
        maxima.buffered_response_bytes(),
        maxima.unacknowledged_stream_items(),
        maxima.workloads_per_page(),
        maxima.log_records_per_page(),
        maxima.encoded_log_message_bytes(),
        maxima.unary_deadline_ms(),
        maxima.lifecycle_deadline_ms(),
    )
    .unwrap();
    let hello = handshake_with_limits(&mut stream, limits);
    for value in [5_u64, 5, 6] {
        send_client_frame(
            &mut stream,
            &ClientFrame::Request(Request {
                server_connection_id: hello.server_connection_id,
                request_id: RequestIdentifier::new(value).unwrap(),
                deadline_ms: Some(5_000),
                operation: SemanticRequest::MockUnary { delay_ms: 500 },
            }),
        );
    }

    let conflict = read_server_frame::<ServerFrame>(&mut stream);
    let overloaded = read_server_frame::<ServerFrame>(&mut stream);

    assert!(matches!(
        conflict,
        ServerFrame::Response(response)
            if matches!(&response.result, ResponseResult::Error(error)
                if error.code == graft::worker::protocol::WorkerErrorCode::RequestConflict)
    ));
    assert!(matches!(
        overloaded,
        ServerFrame::Response(response)
            if matches!(&response.result, ResponseResult::Error(error)
                if error.code == graft::worker::protocol::WorkerErrorCode::Overloaded)
    ));
}

#[cfg(feature = "worker-test-fixtures")]
#[test]
fn sigterm_ends_active_stream_with_worker_shutdown() {
    let mut worker = spawn_worker();
    let mut stream = connect(&worker.socket_path);
    let hello = handshake(&mut stream);
    send_client_frame(
        &mut stream,
        &ClientFrame::Request(Request {
            server_connection_id: hello.server_connection_id,
            request_id: RequestIdentifier::new(7).unwrap(),
            deadline_ms: Some(5_000),
            operation: SemanticRequest::MockStream {
                items: 10,
                interval_ms: 100,
            },
        }),
    );
    assert!(matches!(
        read_server_frame::<ServerFrame>(&mut stream),
        ServerFrame::StreamItem(item) if item.sequence == 1
    ));
    let pid = rustix::process::Pid::from_raw(i32::try_from(worker.child.id()).unwrap()).unwrap();
    rustix::process::kill_process(pid, rustix::process::Signal::TERM).unwrap();

    let terminal = read_server_frame::<ServerFrame>(&mut stream);

    assert!(matches!(
        terminal,
        ServerFrame::StreamEnd(end) if end.reason == StreamEndReason::WorkerShutdown
    ));
    assert!(worker.child.wait().unwrap().success());
}

#[cfg(feature = "worker-test-fixtures")]
#[test]
fn unacknowledged_window_ends_slow_consumer_without_unbounded_output() {
    let worker = spawn_worker();
    let mut stream = connect(&worker.socket_path);
    let maxima = EffectiveLimits::protocol_maxima();
    let limits = EffectiveLimits::new(
        maxima.concurrent_requests(),
        maxima.active_streams(),
        maxima.buffered_response_bytes(),
        1,
        maxima.workloads_per_page(),
        maxima.log_records_per_page(),
        maxima.encoded_log_message_bytes(),
        maxima.unary_deadline_ms(),
        maxima.lifecycle_deadline_ms(),
    )
    .unwrap();
    let hello = handshake_with_limits(&mut stream, limits);
    send_client_frame(
        &mut stream,
        &ClientFrame::Request(Request {
            server_connection_id: hello.server_connection_id,
            request_id: RequestIdentifier::new(8).unwrap(),
            deadline_ms: Some(5_000),
            operation: SemanticRequest::MockStream {
                items: 2,
                interval_ms: 0,
            },
        }),
    );
    assert!(matches!(
        read_server_frame::<ServerFrame>(&mut stream),
        ServerFrame::StreamItem(item) if item.sequence == 1
    ));

    let terminal = read_server_frame::<ServerFrame>(&mut stream);

    assert!(matches!(
        terminal,
        ServerFrame::StreamEnd(end)
            if end.reason == StreamEndReason::SlowConsumer && end.final_sequence == 1
    ));
}

#[cfg(feature = "worker-test-fixtures")]
#[test]
fn duplicate_request_and_wrong_connection_identifiers_fail_typed() {
    let worker = spawn_worker();
    let mut stream = connect(&worker.socket_path);
    let hello = handshake(&mut stream);
    let request_id = RequestIdentifier::new(9).unwrap();
    let request = ClientFrame::Request(Request {
        server_connection_id: hello.server_connection_id,
        request_id,
        deadline_ms: Some(5_000),
        operation: SemanticRequest::MockUnary { delay_ms: 500 },
    });
    send_client_frame(&mut stream, &request);
    send_client_frame(
        &mut stream,
        &ClientFrame::Request(Request {
            server_connection_id: hello.server_connection_id,
            request_id,
            deadline_ms: Some(0),
            operation: SemanticRequest::MockUnary { delay_ms: 0 },
        }),
    );
    let invalid = read_server_frame::<ServerFrame>(&mut stream);
    assert!(matches!(
        invalid,
        ServerFrame::Response(response)
            if matches!(&response.result, ResponseResult::Error(error)
                if error.code == graft::worker::protocol::WorkerErrorCode::Deadline)
    ));

    send_client_frame(&mut stream, &request);
    let conflict = read_server_frame::<ServerFrame>(&mut stream);
    assert!(matches!(
        conflict,
        ServerFrame::Response(response)
            if matches!(&response.result, ResponseResult::Error(error)
                if error.code == graft::worker::protocol::WorkerErrorCode::RequestConflict)
    ));

    send_client_frame(
        &mut stream,
        &ClientFrame::Request(Request {
            server_connection_id: ConnectionIdentifier::parse(
                "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c21",
            )
            .unwrap(),
            request_id: RequestIdentifier::new(10).unwrap(),
            deadline_ms: Some(5_000),
            operation: SemanticRequest::MockUnary { delay_ms: 0 },
        }),
    );
    let mismatch = read_server_frame::<ServerFrame>(&mut stream);
    assert!(matches!(
        mismatch,
        ServerFrame::Response(response)
            if matches!(&response.result, ResponseResult::Error(error)
                if error.code == graft::worker::protocol::WorkerErrorCode::ConnectionMismatch)
    ));
}

#[cfg(feature = "worker-test-fixtures")]
#[test]
fn invalid_stream_acknowledgement_returns_typed_error_and_stops_interest() {
    let worker = spawn_worker();
    let mut stream = connect(&worker.socket_path);
    let maxima = EffectiveLimits::protocol_maxima();
    let limits = EffectiveLimits::new(
        maxima.concurrent_requests(),
        maxima.active_streams(),
        maxima.buffered_response_bytes(),
        1,
        maxima.workloads_per_page(),
        maxima.log_records_per_page(),
        maxima.encoded_log_message_bytes(),
        maxima.unary_deadline_ms(),
        maxima.lifecycle_deadline_ms(),
    )
    .unwrap();
    let hello = handshake_with_limits(&mut stream, limits);
    let request_id = RequestIdentifier::new(11).unwrap();
    send_client_frame(
        &mut stream,
        &ClientFrame::Request(Request {
            server_connection_id: hello.server_connection_id,
            request_id,
            deadline_ms: Some(5_000),
            operation: SemanticRequest::MockStream {
                items: 3,
                interval_ms: 50,
            },
        }),
    );
    assert!(matches!(
        read_server_frame::<ServerFrame>(&mut stream),
        ServerFrame::StreamItem(item) if item.sequence == 1
    ));
    send_client_frame(
        &mut stream,
        &ClientFrame::StreamAck(StreamAck {
            server_connection_id: hello.server_connection_id,
            request_id,
            sequence: 2,
        }),
    );

    let frames = [
        read_server_frame::<ServerFrame>(&mut stream),
        read_server_frame::<ServerFrame>(&mut stream),
    ];

    assert!(frames.iter().any(|frame| matches!(
        frame,
        ServerFrame::Response(response)
            if matches!(&response.result, ResponseResult::Error(error)
                if error.code == graft::worker::protocol::WorkerErrorCode::InvalidAcknowledgement)
    )));
    assert!(frames.iter().any(|frame| matches!(
        frame,
        ServerFrame::StreamEnd(end) if end.reason == StreamEndReason::Cancelled
    )));
}

#[cfg(feature = "worker-test-fixtures")]
#[test]
fn acknowledgement_only_update_preserves_stream_interval_and_completion() {
    let worker = spawn_worker();
    let mut stream = connect(&worker.socket_path);
    let hello = handshake(&mut stream);
    let request_id = RequestIdentifier::new(12).unwrap();
    send_client_frame(
        &mut stream,
        &ClientFrame::Request(Request {
            server_connection_id: hello.server_connection_id,
            request_id,
            deadline_ms: Some(5_000),
            operation: SemanticRequest::MockStream {
                items: 2,
                interval_ms: 50,
            },
        }),
    );
    assert!(matches!(
        read_server_frame::<ServerFrame>(&mut stream),
        ServerFrame::StreamItem(item) if item.sequence == 1
    ));
    send_client_frame(
        &mut stream,
        &ClientFrame::StreamAck(StreamAck {
            server_connection_id: hello.server_connection_id,
            request_id,
            sequence: 1,
        }),
    );

    assert!(matches!(
        read_server_frame::<ServerFrame>(&mut stream),
        ServerFrame::StreamItem(item) if item.sequence == 2
    ));
    assert!(matches!(
        read_server_frame::<ServerFrame>(&mut stream),
        ServerFrame::StreamEnd(end) if end.reason == StreamEndReason::Completed
    ));
}

#[cfg(feature = "worker-test-fixtures")]
#[test]
fn request_deadline_wins_while_stream_ack_window_is_full() {
    let worker = spawn_worker();
    let mut stream = connect(&worker.socket_path);
    let maxima = EffectiveLimits::protocol_maxima();
    let limits = EffectiveLimits::new(
        maxima.concurrent_requests(),
        maxima.active_streams(),
        maxima.buffered_response_bytes(),
        1,
        maxima.workloads_per_page(),
        maxima.log_records_per_page(),
        maxima.encoded_log_message_bytes(),
        maxima.unary_deadline_ms(),
        maxima.lifecycle_deadline_ms(),
    )
    .unwrap();
    let hello = handshake_with_limits(&mut stream, limits);
    send_client_frame(
        &mut stream,
        &ClientFrame::Request(Request {
            server_connection_id: hello.server_connection_id,
            request_id: RequestIdentifier::new(13).unwrap(),
            deadline_ms: Some(250),
            operation: SemanticRequest::MockStream {
                items: 2,
                interval_ms: 0,
            },
        }),
    );
    assert!(matches!(
        read_server_frame::<ServerFrame>(&mut stream),
        ServerFrame::StreamItem(item) if item.sequence == 1
    ));

    assert!(matches!(
        read_server_frame::<ServerFrame>(&mut stream),
        ServerFrame::StreamEnd(end) if end.reason == StreamEndReason::Deadline
    ));
}

#[cfg(feature = "worker-test-fixtures")]
#[test]
fn negotiated_response_byte_exhaustion_closes_connection_deterministically() {
    let worker = spawn_worker();
    let mut stream = connect(&worker.socket_path);
    let maxima = EffectiveLimits::protocol_maxima();
    let limits = EffectiveLimits::new(
        maxima.concurrent_requests(),
        maxima.active_streams(),
        1,
        maxima.unacknowledged_stream_items(),
        maxima.workloads_per_page(),
        maxima.log_records_per_page(),
        maxima.encoded_log_message_bytes(),
        maxima.unary_deadline_ms(),
        maxima.lifecycle_deadline_ms(),
    )
    .unwrap();
    let hello = handshake_with_limits(&mut stream, limits);
    send_client_frame(
        &mut stream,
        &ClientFrame::Request(Request {
            server_connection_id: hello.server_connection_id,
            request_id: RequestIdentifier::new(14).unwrap(),
            deadline_ms: Some(1_000),
            operation: SemanticRequest::MockUnary { delay_ms: 0 },
        }),
    );
    let mut byte = [0_u8; 1];
    assert_eq!(stream.read(&mut byte).unwrap(), 0);

    stream.set_nonblocking(true).unwrap();
    let closed_at = Instant::now() + Duration::from_secs(1);
    loop {
        match stream.write(&[0_u8; 4]) {
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::BrokenPipe
                        | std::io::ErrorKind::ConnectionReset
                        | std::io::ErrorKind::NotConnected
                ) =>
            {
                break;
            }
            Ok(_) | Err(_) if Instant::now() < closed_at => {
                thread::sleep(Duration::from_millis(10));
            }
            result => panic!("worker retained read half after writer termination: {result:?}"),
        }
    }
}

#[cfg(feature = "worker-test-fixtures")]
#[test]
fn no_progress_acknowledgements_do_not_extend_fixed_stall_deadline() {
    let worker = spawn_worker();
    let mut stream = connect(&worker.socket_path);
    let maxima = EffectiveLimits::protocol_maxima();
    let limits = EffectiveLimits::new(
        maxima.concurrent_requests(),
        maxima.active_streams(),
        maxima.buffered_response_bytes(),
        1,
        maxima.workloads_per_page(),
        maxima.log_records_per_page(),
        maxima.encoded_log_message_bytes(),
        maxima.unary_deadline_ms(),
        maxima.lifecycle_deadline_ms(),
    )
    .unwrap();
    let hello = handshake_with_limits(&mut stream, limits);
    let request_id = RequestIdentifier::new(15).unwrap();
    send_client_frame(
        &mut stream,
        &ClientFrame::Request(Request {
            server_connection_id: hello.server_connection_id,
            request_id,
            deadline_ms: Some(5_000),
            operation: SemanticRequest::MockStream {
                items: 2,
                interval_ms: 0,
            },
        }),
    );
    assert!(matches!(
        read_server_frame::<ServerFrame>(&mut stream),
        ServerFrame::StreamItem(item) if item.sequence == 1
    ));
    let mut acknowledger = stream.try_clone().unwrap();
    let started = Instant::now();
    let sender = thread::spawn(move || {
        for _ in 0..5 {
            thread::sleep(Duration::from_millis(100));
            send_client_frame(
                &mut acknowledger,
                &ClientFrame::StreamAck(StreamAck {
                    server_connection_id: hello.server_connection_id,
                    request_id,
                    sequence: 0,
                }),
            );
        }
    });

    let terminal = read_server_frame::<ServerFrame>(&mut stream);
    sender.join().unwrap();

    assert!(matches!(
        terminal,
        ServerFrame::StreamEnd(end) if end.reason == StreamEndReason::SlowConsumer
    ));
    assert!(started.elapsed() < Duration::from_millis(1_500));
}
