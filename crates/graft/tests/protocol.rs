use serde::ser::SerializeSeq as _;

use graft::protocol::{
    decode_frame, encode_frame, negotiate_handshake, validate_server_hello, Capability,
    CapabilitySet, ClientComponent, ClientHandshakeFrame, ClientHello, CodecError,
    ConnectionIdentifier, EffectiveLimits, FrameDirection, HandshakeError, ManagerKind,
    ManifestGeneration, ManifestState, ProtocolVersionRange, RequestIdentifier, SafeSummary,
    ServerHandshakeConfig, ServerHandshakeFrame, ServerTimeMilliseconds, SoftwareVersion,
    WorkerContext, WorkerTarget, MAX_INBOUND_FRAME_BYTES, MAX_JSON_INTEGER,
    MAX_OUTBOUND_FRAME_BYTES, MAX_SAFE_SUMMARY_BYTES, MAX_SOFTWARE_VERSION_BYTES,
};

const CLIENT_ID: &str = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20";
const SERVER_ID: &str = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c21";
const WORKER_EPOCH: &str = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c22";
const GENERATION: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn capabilities(values: &[Capability]) -> CapabilitySet {
    CapabilitySet::new(values.iter().copied()).expect("fixture capabilities are unique")
}

fn client_hello() -> ClientHello {
    ClientHello {
        protocol: ProtocolVersionRange::new(1, 0, 0).expect("fixture range is valid"),
        component: ClientComponent::Cli,
        software_version: SoftwareVersion::parse("0.3.0-alpha.1")
            .expect("fixture version is valid"),
        requested_capabilities: capabilities(&[Capability::Observe, Capability::Inspect]),
        requested_limits: EffectiveLimits::protocol_maxima(),
        client_connection_id: ConnectionIdentifier::parse(CLIENT_ID)
            .expect("fixture identifier is valid"),
    }
}

fn server_config() -> ServerHandshakeConfig {
    ServerHandshakeConfig {
        protocol: ProtocolVersionRange::new(1, 0, 0).expect("fixture range is valid"),
        software_version: SoftwareVersion::parse("0.3.0-alpha.1")
            .expect("fixture version is valid"),
        context: WorkerContext::new(WorkerTarget::User, 1000, ManagerKind::User)
            .expect("fixture context is valid"),
        capabilities: capabilities(&[
            Capability::Observe,
            Capability::Inspect,
            Capability::Metrics,
        ]),
        limits: EffectiveLimits::protocol_maxima(),
        manifest: ManifestState::Available {
            generation: ManifestGeneration::parse(GENERATION).expect("fixture generation is valid"),
        },
        worker_epoch: ConnectionIdentifier::parse(WORKER_EPOCH)
            .expect("fixture identifier is valid"),
        server_time_ms: ServerTimeMilliseconds::new(1_700_000_000_000)
            .expect("fixture server time is valid"),
        server_connection_id: ConnectionIdentifier::parse(SERVER_ID)
            .expect("fixture identifier is valid"),
    }
}

#[test]
fn client_hello_round_trip_preserves_typed_fields() {
    let original = ClientHandshakeFrame::ClientHello(client_hello());

    let encoded =
        encode_frame(&original, FrameDirection::ClientToServer).expect("valid hello should encode");
    let decoded: ClientHandshakeFrame =
        decode_frame(&encoded, FrameDirection::ClientToServer).expect("valid hello should decode");

    assert_eq!(decoded, original);
    assert_eq!(
        usize::try_from(u32::from_be_bytes(encoded[..4].try_into().unwrap())).unwrap(),
        encoded.len() - 4
    );
}

#[test]
fn server_hello_round_trip_uses_negotiated_values() {
    let hello = negotiate_handshake(&client_hello(), &server_config())
        .expect("compatible handshake should succeed");
    let original = ServerHandshakeFrame::ServerHello(hello);

    let encoded =
        encode_frame(&original, FrameDirection::ServerToClient).expect("valid hello should encode");
    let decoded: ServerHandshakeFrame =
        decode_frame(&encoded, FrameDirection::ServerToClient).expect("valid hello should decode");

    assert_eq!(decoded, original);
}

#[test]
fn codec_accepts_exact_inbound_limit_and_rejects_one_extra_byte() {
    let exact = "a".repeat(MAX_INBOUND_FRAME_BYTES - 2);
    let oversized = "a".repeat(MAX_INBOUND_FRAME_BYTES - 1);

    let encoded = encode_frame(&exact, FrameDirection::ClientToServer)
        .expect("exact-limit JSON string should encode");
    let error = encode_frame(&oversized, FrameDirection::ClientToServer)
        .expect_err("one extra payload byte should fail");

    assert_eq!(encoded.len(), MAX_INBOUND_FRAME_BYTES + 4);
    assert!(matches!(
        error,
        CodecError::Oversized {
            actual,
            maximum: MAX_INBOUND_FRAME_BYTES
        } if actual == MAX_INBOUND_FRAME_BYTES + 1
    ));
}

#[test]
fn bounded_encoder_stops_incremental_serialization_at_directional_limit() {
    let value = RepeatedItems(100_000);

    let error = encode_frame(&value, FrameDirection::ClientToServer)
        .expect_err("incremental serialization must stop at the inbound limit");

    assert!(matches!(
        error,
        CodecError::Oversized {
            actual,
            maximum: MAX_INBOUND_FRAME_BYTES
        } if actual > MAX_INBOUND_FRAME_BYTES
    ));
}

#[test]
fn codec_applies_distinct_outbound_limit_and_rejects_oversized_prefix_early() {
    let outbound_exact = "a".repeat(MAX_OUTBOUND_FRAME_BYTES - 2);
    let encoded = encode_frame(&outbound_exact, FrameDirection::ServerToClient)
        .expect("exact outbound limit should encode");
    let declared_oversized = u32::try_from(MAX_INBOUND_FRAME_BYTES + 1)
        .unwrap()
        .to_be_bytes();

    assert_eq!(encoded.len(), MAX_OUTBOUND_FRAME_BYTES + 4);
    assert!(matches!(
        decode_frame::<ClientHandshakeFrame>(
            &declared_oversized,
            FrameDirection::ClientToServer
        ),
        Err(CodecError::Oversized {
            actual,
            maximum: MAX_INBOUND_FRAME_BYTES
        }) if actual == MAX_INBOUND_FRAME_BYTES + 1
    ));
}

#[test]
fn codec_rejects_zero_truncated_trailing_and_invalid_utf8_frames() {
    let zero = 0_u32.to_be_bytes();
    let truncated_payload = [3_u32.to_be_bytes().as_slice(), b"{}"].concat();
    let trailing = [2_u32.to_be_bytes().as_slice(), b"{}x"].concat();
    let invalid_utf8 = [1_u32.to_be_bytes().as_slice(), &[0xff]].concat();

    assert!(matches!(
        decode_frame::<serde_json::Value>(&[0, 0, 0], FrameDirection::ClientToServer),
        Err(CodecError::TruncatedPrefix)
    ));
    assert!(matches!(
        decode_frame::<serde_json::Value>(&zero, FrameDirection::ClientToServer),
        Err(CodecError::ZeroLength)
    ));
    assert!(matches!(
        decode_frame::<serde_json::Value>(&truncated_payload, FrameDirection::ClientToServer),
        Err(CodecError::TruncatedPayload {
            declared: 3,
            received: 2
        })
    ));
    assert!(matches!(
        decode_frame::<serde_json::Value>(&trailing, FrameDirection::ClientToServer),
        Err(CodecError::TrailingBytes { count: 1 })
    ));
    assert!(matches!(
        decode_frame::<serde_json::Value>(&invalid_utf8, FrameDirection::ClientToServer),
        Err(CodecError::InvalidUtf8(_))
    ));
}

#[test]
fn typed_decode_rejects_unknown_duplicate_and_unknown_enum_fields() {
    let valid = serde_json::to_value(ClientHandshakeFrame::ClientHello(client_hello())).unwrap();
    let mut unknown = valid.clone();
    unknown
        .as_object_mut()
        .unwrap()
        .insert("unexpected".to_string(), true.into());
    let unknown_frame = encode_json_value(&unknown);
    let mut unknown_protocol = valid.clone();
    unknown_protocol["protocol"]["unexpected"] = true.into();
    let unknown_protocol_frame = encode_json_value(&unknown_protocol);
    let mut unknown_limits = valid.clone();
    unknown_limits["requested_limits"]["unexpected"] = true.into();
    let unknown_limits_frame = encode_json_value(&unknown_limits);

    let duplicate_json = serde_json::to_string(&valid).unwrap().replacen(
        "\"component\":",
        "\"component\":\"cli\",\"component\":",
        1,
    );
    let duplicate_frame = raw_frame(duplicate_json.as_bytes());

    let unknown_enum_json = serde_json::to_string(&valid)
        .unwrap()
        .replace("\"component\":\"cli\"", "\"component\":\"daemon\"");
    let unknown_enum_frame = raw_frame(unknown_enum_json.as_bytes());

    for frame in [
        &unknown_frame,
        &unknown_protocol_frame,
        &unknown_limits_frame,
        &duplicate_frame,
        &unknown_enum_frame,
    ] {
        assert!(matches!(
            decode_frame::<ClientHandshakeFrame>(frame, FrameDirection::ClientToServer),
            Err(CodecError::Decode)
        ));
    }
}

#[test]
fn typed_decode_rejects_reversed_range_duplicate_capability_and_excessive_limit() {
    let valid = serde_json::to_value(ClientHandshakeFrame::ClientHello(client_hello())).unwrap();

    let mut reversed = valid.clone();
    reversed["protocol"]["min_minor"] = 1.into();
    let mut duplicated = valid.clone();
    duplicated["requested_capabilities"] = serde_json::json!(["observe", "observe"]);
    let mut excessive = valid;
    excessive["requested_limits"]["concurrent_requests"] = 33.into();

    for value in [reversed, duplicated, excessive] {
        assert!(matches!(
            decode_frame::<ClientHandshakeFrame>(
                &encode_json_value(&value),
                FrameDirection::ClientToServer
            ),
            Err(CodecError::Decode)
        ));
    }
}

#[test]
fn negotiation_selects_highest_minor_and_intersects_limits() {
    let mut client = client_hello();
    client.protocol = ProtocolVersionRange::new(1, 1, 4).unwrap();
    client.requested_limits =
        EffectiveLimits::new(16, 4, 1_048_576, 32, 100, 500, 32_768, 30_000, 120_000).unwrap();
    let mut server = server_config();
    server.protocol = ProtocolVersionRange::new(1, 2, 3).unwrap();
    server.limits =
        EffectiveLimits::new(8, 8, 2_097_152, 64, 50, 1_000, 65_536, 60_000, 300_000).unwrap();

    let selected = negotiate_handshake(&client, &server).unwrap();

    assert_eq!(selected.protocol.major(), 1);
    assert_eq!(selected.protocol.minor(), 3);
    assert!(selected.capabilities.contains(Capability::Metrics));
    assert_eq!(selected.effective_limits.concurrent_requests(), 8);
    assert_eq!(selected.effective_limits.active_streams(), 4);
    assert_eq!(
        selected.effective_limits.buffered_response_bytes(),
        1_048_576
    );
    assert_eq!(selected.effective_limits.unacknowledged_stream_items(), 32);
    assert_eq!(selected.effective_limits.workloads_per_page(), 50);
    assert_eq!(selected.effective_limits.log_records_per_page(), 500);
    assert_eq!(
        selected.effective_limits.encoded_log_message_bytes(),
        32_768
    );
    assert_eq!(selected.effective_limits.unary_deadline_ms(), 30_000);
    assert_eq!(selected.effective_limits.lifecycle_deadline_ms(), 120_000);
}

#[test]
fn negotiation_rejects_major_minor_and_capability_mismatch() {
    let client = client_hello();

    let mut wrong_major = server_config();
    wrong_major.protocol = ProtocolVersionRange::new(2, 0, 0).unwrap();
    assert_eq!(
        negotiate_handshake(&client, &wrong_major),
        Err(HandshakeError::UnsupportedVersion)
    );

    let mut wrong_minor = server_config();
    wrong_minor.protocol = ProtocolVersionRange::new(1, 1, 2).unwrap();
    assert_eq!(
        negotiate_handshake(&client, &wrong_minor),
        Err(HandshakeError::UnsupportedVersion)
    );

    let mut missing_capability = server_config();
    missing_capability.capabilities = capabilities(&[Capability::Observe]);
    assert_eq!(
        negotiate_handshake(&client, &missing_capability),
        Err(HandshakeError::UnsupportedCapability(Capability::Inspect))
    );
}

#[test]
fn client_validation_rejects_server_version_capability_and_limit_escalation() {
    let client = client_hello();
    let mut server = negotiate_handshake(&client, &server_config()).unwrap();
    assert_eq!(validate_server_hello(&client, &server), Ok(()));

    server.protocol = graft::protocol::ProtocolVersion::new(1, 1);
    assert_eq!(
        validate_server_hello(&client, &server),
        Err(HandshakeError::InvalidServerVersion)
    );

    server = negotiate_handshake(&client, &server_config()).unwrap();
    server.capabilities = capabilities(&[Capability::Observe]);
    assert_eq!(
        validate_server_hello(&client, &server),
        Err(HandshakeError::MissingServerCapability(Capability::Inspect))
    );

    let mut limited_client = client_hello();
    limited_client.requested_limits = EffectiveLimits::new(1, 1, 1, 1, 1, 1, 1, 1, 1).unwrap();
    server = negotiate_handshake(&client, &server_config()).unwrap();
    assert_eq!(
        validate_server_hello(&limited_client, &server),
        Err(HandshakeError::InvalidServerLimits)
    );
}

#[test]
fn request_identifier_enforces_non_zero_json_integer_boundaries_and_serde() {
    let minimum = RequestIdentifier::new(1).expect("one is a valid request identifier");
    let maximum =
        RequestIdentifier::new(MAX_JSON_INTEGER).expect("maximum interoperable integer is valid");

    assert_eq!(minimum.get(), 1);
    assert_eq!(maximum.get(), MAX_JSON_INTEGER);
    assert!(RequestIdentifier::new(0).is_err());
    assert!(RequestIdentifier::new(MAX_JSON_INTEGER + 1).is_err());
    assert_eq!(serde_json::to_string(&minimum).unwrap(), "1");
    assert_eq!(
        serde_json::from_str::<RequestIdentifier>("1").unwrap(),
        minimum
    );
    assert!(serde_json::from_str::<RequestIdentifier>("0").is_err());
    assert!(
        serde_json::from_str::<RequestIdentifier>(&(MAX_JSON_INTEGER + 1).to_string()).is_err()
    );
}

#[test]
fn server_time_rejects_integer_above_json_interoperable_range() {
    assert!(ServerTimeMilliseconds::new(MAX_JSON_INTEGER).is_ok());
    assert!(ServerTimeMilliseconds::new(MAX_JSON_INTEGER + 1).is_err());
}

#[test]
fn bounded_text_and_identity_types_reject_invalid_boundary_values() {
    assert!(SoftwareVersion::parse("v").is_ok());
    assert!(SoftwareVersion::parse("").is_err());
    assert!(SoftwareVersion::parse("a".repeat(MAX_SOFTWARE_VERSION_BYTES + 1)).is_err());
    assert!(SoftwareVersion::parse("bad\nversion").is_err());

    assert!(SafeSummary::parse("safe").is_ok());
    assert!(SafeSummary::parse("").is_err());
    assert!(SafeSummary::parse("a".repeat(MAX_SAFE_SUMMARY_BYTES + 1)).is_err());
    assert!(SafeSummary::parse("unsafe\rsummary").is_err());

    assert!(ConnectionIdentifier::parse(CLIENT_ID).is_ok());
    assert!(ConnectionIdentifier::parse(&CLIENT_ID.to_uppercase()).is_err());
    assert!(ConnectionIdentifier::parse("018f0f77-8c4d-4b2a-8e6a-4b8a7d3a1c20").is_err());
    assert!(ConnectionIdentifier::parse("018f0f77-8c4d-7b2a-1e6a-4b8a7d3a1c20").is_err());

    assert!(ManifestGeneration::parse(GENERATION).is_ok());
    assert!(ManifestGeneration::parse(GENERATION.to_uppercase()).is_err());
    assert!(ManifestGeneration::parse("abc").is_err());
}

#[test]
fn worker_context_rejects_cross_manager_and_non_root_system_identity() {
    assert!(WorkerContext::new(WorkerTarget::System, 0, ManagerKind::System).is_ok());
    assert!(WorkerContext::new(WorkerTarget::User, 0, ManagerKind::User).is_ok());
    assert!(WorkerContext::new(WorkerTarget::User, 1000, ManagerKind::User).is_ok());
    assert!(WorkerContext::new(WorkerTarget::System, 1000, ManagerKind::System).is_err());
    assert!(WorkerContext::new(WorkerTarget::System, 0, ManagerKind::User).is_err());
    assert!(WorkerContext::new(WorkerTarget::User, 1000, ManagerKind::System).is_err());
}

#[test]
fn codec_error_does_not_echo_malformed_payload() {
    let secret = b"credential=do-not-echo";
    let frame = raw_frame(secret);

    let error = decode_frame::<ClientHandshakeFrame>(&frame, FrameDirection::ClientToServer)
        .expect_err("malformed payload should fail");
    let diagnostic = error.to_string();

    assert!(!diagnostic.contains("credential"));
    assert!(!diagnostic.contains("do-not-echo"));
}

struct RepeatedItems(usize);

impl serde::Serialize for RepeatedItems {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut sequence = serializer.serialize_seq(None)?;
        for _ in 0..self.0 {
            sequence.serialize_element("x")?;
        }
        sequence.end()
    }
}

fn raw_frame(payload: &[u8]) -> Vec<u8> {
    let mut frame = Vec::with_capacity(payload.len() + 4);
    frame.extend_from_slice(&u32::try_from(payload.len()).unwrap().to_be_bytes());
    frame.extend_from_slice(payload);
    frame
}

fn encode_json_value(value: &serde_json::Value) -> Vec<u8> {
    raw_frame(&serde_json::to_vec(value).unwrap())
}
