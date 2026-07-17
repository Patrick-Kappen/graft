//! Per-process worker epoch and monotone logical wall-time mapping.

use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::protocol::{ConnectionIdentifier, ServerTimeMilliseconds, ValidationError};

/// Immutable identity and logical-time origin for one worker process.
#[derive(Debug, Clone)]
pub struct WorkerEpoch {
    identifier: ConnectionIdentifier,
    wall_anchor_ms: u64,
    monotonic_origin: Instant,
}

impl WorkerEpoch {
    /// Creates a fresh process epoch.
    ///
    /// # Errors
    ///
    /// Returns an error when wall time precedes the Unix epoch or cannot fit the
    /// interoperable protocol integer range.
    pub fn new() -> Result<Self, ValidationError> {
        let wall_anchor_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| ValidationError::JsonIntegerTooLarge {
                field: "worker epoch wall anchor",
            })?
            .as_millis();
        let wall_anchor_ms =
            u64::try_from(wall_anchor_ms).map_err(|_| ValidationError::JsonIntegerTooLarge {
                field: "worker epoch wall anchor",
            })?;
        ServerTimeMilliseconds::new(wall_anchor_ms)?;
        Ok(Self {
            identifier: ConnectionIdentifier::from_uuid(uuid::Uuid::now_v7())?,
            wall_anchor_ms,
            monotonic_origin: Instant::now(),
        })
    }

    /// Returns this process's immutable epoch identifier.
    #[must_use]
    pub const fn identifier(&self) -> ConnectionIdentifier {
        self.identifier
    }

    /// Returns logical receive time derived from the fixed wall anchor.
    ///
    /// # Errors
    ///
    /// Returns an error only if process uptime would exceed protocol integer
    /// bounds.
    pub fn logical_now(&self) -> Result<ServerTimeMilliseconds, ValidationError> {
        let elapsed = u64::try_from(self.monotonic_origin.elapsed().as_millis()).map_err(|_| {
            ValidationError::JsonIntegerTooLarge {
                field: "worker epoch elapsed time",
            }
        })?;
        ServerTimeMilliseconds::new(self.wall_anchor_ms.saturating_add(elapsed))
    }
}
