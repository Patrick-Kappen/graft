//! Versioned typed local worker protocol foundation.
//!
//! This module contains only pure framing, handshake, and validation logic. It
//! does not open sockets, select worker authority, or dispatch backend work.

mod codec;
mod handshake;
mod types;

pub use codec::{
    decode_frame, encode_frame, CodecError, FrameDirection, MAX_INBOUND_FRAME_BYTES,
    MAX_OUTBOUND_FRAME_BYTES,
};
pub use handshake::{
    negotiate_handshake, validate_server_hello, ClientHandshakeFrame, ClientHello, EffectiveLimits,
    HandshakeError, ManifestState, ManifestUnavailableReason, ProtocolError, ProtocolErrorCode,
    ProtocolMaxima, ServerHandshakeConfig, ServerHandshakeFrame, ServerHello, WorkerContext,
};
pub use types::{
    Capability, CapabilitySet, ClientComponent, ConnectionIdentifier, ManagerKind,
    ManifestGeneration, ProtocolVersion, ProtocolVersionRange, RequestIdentifier, SafeSummary,
    ServerTimeMilliseconds, SoftwareVersion, ValidationError, WorkerTarget, MAX_JSON_INTEGER,
    MAX_SAFE_SUMMARY_BYTES, MAX_SOFTWARE_VERSION_BYTES, PROTOCOL_MAJOR, PROTOCOL_MAX_MINOR,
    PROTOCOL_MIN_MINOR,
};
