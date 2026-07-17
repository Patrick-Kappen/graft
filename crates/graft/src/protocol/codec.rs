//! Direction-bounded length-prefixed JSON frame codec.

use std::io;

use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;

/// Fixed maximum client-to-worker payload bytes.
pub const MAX_INBOUND_FRAME_BYTES: usize = 64 * 1024;
/// Fixed maximum worker-to-client payload bytes.
pub const MAX_OUTBOUND_FRAME_BYTES: usize = 256 * 1024;
const PREFIX_BYTES: usize = 4;

/// Frame direction selecting the fixed payload limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameDirection {
    /// Client to worker.
    ClientToServer,
    /// Worker to client.
    ServerToClient,
}

impl FrameDirection {
    const fn maximum(self) -> usize {
        match self {
            Self::ClientToServer => MAX_INBOUND_FRAME_BYTES,
            Self::ServerToClient => MAX_OUTBOUND_FRAME_BYTES,
        }
    }
}

/// Typed error returned by frame encoding or decoding.
#[derive(Debug, Error)]
pub enum CodecError {
    /// Serialization failed before any frame was emitted.
    #[error("failed to encode JSON payload")]
    Encode(#[source] serde_json::Error),
    /// Input has fewer than four prefix bytes.
    #[error("frame length prefix is truncated")]
    TruncatedPrefix,
    /// Declared payload length is zero.
    #[error("frame payload length must not be zero")]
    ZeroLength,
    /// Declared or encoded payload exceeds the directional maximum.
    #[error("frame payload length {actual} exceeds {maximum} bytes")]
    Oversized {
        /// Actual or declared payload byte count.
        actual: usize,
        /// Directional payload maximum.
        maximum: usize,
    },
    /// Input ends before the complete declared payload.
    #[error("frame payload is truncated: declared {declared} bytes, received {received}")]
    TruncatedPayload {
        /// Declared payload byte count.
        declared: usize,
        /// Received payload byte count.
        received: usize,
    },
    /// Input contains bytes after the one declared frame.
    #[error("frame contains {count} trailing bytes")]
    TrailingBytes {
        /// Number of bytes after the declared frame.
        count: usize,
    },
    /// Payload is not valid UTF-8.
    #[error("frame payload is not valid UTF-8")]
    InvalidUtf8(#[source] std::str::Utf8Error),
    /// Payload does not match the requested typed JSON schema.
    #[error("frame payload is not valid typed JSON")]
    Decode,
}

/// Encodes one typed JSON payload with a four-byte big-endian length prefix.
///
/// # Errors
///
/// Returns an error when serialization fails, produces an empty payload, or
/// exceeds the fixed directional maximum.
pub fn encode_frame<T: Serialize>(
    value: &T,
    direction: FrameDirection,
) -> Result<Vec<u8>, CodecError> {
    let maximum = direction.maximum();
    let mut writer = BoundedFrameWriter::new(maximum, PREFIX_BYTES + maximum.min(4_096));
    if let Err(source) = serde_json::to_writer(&mut writer, value) {
        return match writer.exceeded {
            Some(actual) => Err(CodecError::Oversized { actual, maximum }),
            None => Err(CodecError::Encode(source)),
        };
    }
    writer.finish()
}

/// Counts the exact encoded frame length without allocating the payload.
///
/// # Errors
///
/// Returns an error when serialization fails or exceeds the directional bound.
pub fn encoded_frame_len<T: Serialize>(
    value: &T,
    direction: FrameDirection,
) -> Result<usize, CodecError> {
    let maximum = direction.maximum();
    let mut writer = CountingWriter {
        length: 0,
        maximum,
        exceeded: None,
    };
    if let Err(source) = serde_json::to_writer(&mut writer, value) {
        return match writer.exceeded {
            Some(actual) => Err(CodecError::Oversized { actual, maximum }),
            None => Err(CodecError::Encode(source)),
        };
    }
    if writer.length == 0 {
        return Err(CodecError::ZeroLength);
    }
    Ok(PREFIX_BYTES + writer.length)
}

/// Encodes into one allocation sized by a prior [`encoded_frame_len`] call.
///
/// # Errors
///
/// Returns an error if serialization fails or differs from the expected size.
pub fn encode_frame_exact<T: Serialize>(
    value: &T,
    direction: FrameDirection,
    expected_length: usize,
) -> Result<Vec<u8>, CodecError> {
    let maximum = direction.maximum();
    let mut writer = BoundedFrameWriter::new(maximum, expected_length);
    if let Err(source) = serde_json::to_writer(&mut writer, value) {
        return match writer.exceeded {
            Some(actual) => Err(CodecError::Oversized { actual, maximum }),
            None => Err(CodecError::Encode(source)),
        };
    }
    let frame = writer.finish()?;
    if frame.len() != expected_length {
        return Err(CodecError::Encode(serde_json::Error::io(
            std::io::Error::new(std::io::ErrorKind::InvalidData, "encoded length changed"),
        )));
    }
    Ok(frame)
}

struct CountingWriter {
    length: usize,
    maximum: usize,
    exceeded: Option<usize>,
}

impl io::Write for CountingWriter {
    fn write(&mut self, buffer: &[u8]) -> std::io::Result<usize> {
        let actual = self.length.saturating_add(buffer.len());
        if actual > self.maximum {
            self.exceeded = Some(actual);
            return Err(std::io::Error::other("frame exceeds directional limit"));
        }
        self.length = actual;
        Ok(buffer.len())
    }

    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

struct BoundedFrameWriter {
    frame: Vec<u8>,
    maximum: usize,
    exceeded: Option<usize>,
}

impl BoundedFrameWriter {
    fn new(maximum: usize, capacity: usize) -> Self {
        let mut frame = Vec::with_capacity(capacity);
        frame.extend_from_slice(&[0; PREFIX_BYTES]);
        Self {
            frame,
            maximum,
            exceeded: None,
        }
    }

    fn finish(mut self) -> Result<Vec<u8>, CodecError> {
        let payload_length = self.frame.len() - PREFIX_BYTES;
        if payload_length == 0 {
            return Err(CodecError::ZeroLength);
        }
        let encoded_length = u32::try_from(payload_length).map_err(|_| CodecError::Oversized {
            actual: payload_length,
            maximum: self.maximum,
        })?;
        self.frame[..PREFIX_BYTES].copy_from_slice(&encoded_length.to_be_bytes());
        Ok(self.frame)
    }
}

impl io::Write for BoundedFrameWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        let payload_length = self.frame.len() - PREFIX_BYTES;
        let attempted = payload_length.saturating_add(buffer.len());
        if attempted > self.maximum {
            self.exceeded = Some(attempted);
            return Err(io::Error::other("encoded frame exceeds protocol limit"));
        }
        self.frame.extend_from_slice(buffer);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Decodes exactly one direction-bounded typed JSON frame.
///
/// # Errors
///
/// Returns an error for a truncated/zero/oversized frame, trailing bytes,
/// invalid UTF-8, or JSON that does not match `T`.
pub fn decode_frame<T: DeserializeOwned>(
    frame: &[u8],
    direction: FrameDirection,
) -> Result<T, CodecError> {
    let prefix: [u8; PREFIX_BYTES] = frame
        .get(..PREFIX_BYTES)
        .ok_or(CodecError::TruncatedPrefix)?
        .try_into()
        .map_err(|_| CodecError::TruncatedPrefix)?;
    let declared =
        usize::try_from(u32::from_be_bytes(prefix)).map_err(|_| CodecError::Oversized {
            actual: usize::MAX,
            maximum: direction.maximum(),
        })?;
    if declared == 0 {
        return Err(CodecError::ZeroLength);
    }
    let maximum = direction.maximum();
    if declared > maximum {
        return Err(CodecError::Oversized {
            actual: declared,
            maximum,
        });
    }
    let received = frame.len().saturating_sub(PREFIX_BYTES);
    if received < declared {
        return Err(CodecError::TruncatedPayload { declared, received });
    }
    if received > declared {
        return Err(CodecError::TrailingBytes {
            count: received - declared,
        });
    }
    let payload = &frame[PREFIX_BYTES..];
    std::str::from_utf8(payload).map_err(CodecError::InvalidUtf8)?;
    serde_json::from_slice(payload).map_err(|_| CodecError::Decode)
}
