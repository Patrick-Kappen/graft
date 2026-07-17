//! Incremental bounded asynchronous protocol framing.

use std::io;
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde::Serialize;
use thiserror::Error;
use tokio::io::{AsyncRead, AsyncReadExt as _, AsyncWrite, AsyncWriteExt as _};

use crate::protocol::{encode_frame, CodecError, FrameDirection, MAX_INBOUND_FRAME_BYTES};

/// Maximum time allowed to complete a partially received frame.
pub const PARTIAL_FRAME_TIMEOUT: Duration = Duration::from_secs(30);

/// Async framing failure.
#[derive(Debug, Error)]
pub enum AsyncFrameError {
    /// Transport I/O failed.
    #[error("protocol transport I/O failed")]
    Io(#[source] io::Error),
    /// Partial frame did not complete in time.
    #[error("partial frame deadline elapsed")]
    Timeout,
    /// Peer closed between complete frames.
    #[error("protocol peer disconnected")]
    Disconnected,
    /// Declared frame is empty or oversized.
    #[error("protocol frame length is invalid")]
    Length,
    /// Typed payload is malformed.
    #[error("protocol frame payload is invalid")]
    Decode,
    /// Typed response cannot be encoded.
    #[error("protocol response encoding failed")]
    Encode(#[source] CodecError),
}

/// Reads one exact client-to-server frame without unbounded allocation.
///
/// # Errors
///
/// Returns an error for disconnect, timeout, invalid length, I/O, UTF-8, or
/// typed JSON failure.
pub async fn read_frame<R, T>(reader: &mut R) -> Result<T, AsyncFrameError>
where
    R: AsyncRead + Unpin,
    T: DeserializeOwned,
{
    let mut prefix = [0_u8; 4];
    match reader.read_exact(&mut prefix[..1]).await {
        Ok(_) => {}
        Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => {
            return Err(AsyncFrameError::Disconnected);
        }
        Err(error) => return Err(AsyncFrameError::Io(error)),
    }
    let frame_deadline = tokio::time::Instant::now() + PARTIAL_FRAME_TIMEOUT;
    tokio::time::timeout_at(frame_deadline, reader.read_exact(&mut prefix[1..]))
        .await
        .map_err(|_| AsyncFrameError::Timeout)?
        .map_err(AsyncFrameError::Io)?;
    let length =
        usize::try_from(u32::from_be_bytes(prefix)).map_err(|_| AsyncFrameError::Length)?;
    if length == 0 || length > MAX_INBOUND_FRAME_BYTES {
        return Err(AsyncFrameError::Length);
    }
    let mut payload = vec![0_u8; length];
    tokio::time::timeout_at(frame_deadline, reader.read_exact(&mut payload))
        .await
        .map_err(|_| AsyncFrameError::Timeout)?
        .map_err(AsyncFrameError::Io)?;
    std::str::from_utf8(&payload).map_err(|_| AsyncFrameError::Decode)?;
    serde_json::from_slice(&payload).map_err(|_| AsyncFrameError::Decode)
}

/// Writes one exact server-to-client frame.
///
/// # Errors
///
/// Returns an error when bounded encoding or transport output fails.
pub async fn write_frame<W, T>(writer: &mut W, value: &T) -> Result<(), AsyncFrameError>
where
    W: AsyncWrite + Unpin,
    T: Serialize,
{
    let frame =
        encode_frame(value, FrameDirection::ServerToClient).map_err(AsyncFrameError::Encode)?;
    tokio::time::timeout(PARTIAL_FRAME_TIMEOUT, writer.write_all(&frame))
        .await
        .map_err(|_| AsyncFrameError::Timeout)?
        .map_err(AsyncFrameError::Io)
}
