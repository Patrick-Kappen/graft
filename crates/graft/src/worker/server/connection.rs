use super::*;

pub(super) fn peer_is_authorized(context: WorkerContext, peer_uid: u32) -> bool {
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

pub(super) struct Connection {
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
    pub(super) async fn run(self) {
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
                            capabilities: server_hello.capabilities.clone(),
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
                                if code == WorkerErrorCode::InvalidAcknowledgement {
                                    cancel_stream_interest(&active, control.request_id);
                                }
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
