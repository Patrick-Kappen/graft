//! Typed semantic dispatcher boundary for worker server transports.

use std::future::Future;
use std::pin::Pin;

use crate::protocol::{ConnectionIdentifier, RequestIdentifier, WorkerTarget};

use super::protocol::SemanticRequest;

/// Kernel-authenticated Unix peer credentials captured at accept time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerCredentials {
    /// Kernel peer PID.
    pub pid: i32,
    /// Kernel peer effective UID.
    pub uid: u32,
    /// Kernel peer effective GID.
    pub gid: u32,
}

/// Stable local principal key shared across that UID's connections.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PrincipalKey {
    /// Fixed worker target.
    pub target: WorkerTarget,
    /// Kernel-authenticated peer UID.
    pub uid: u32,
}

/// Authenticated metadata attached to one semantic dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DispatchContext {
    /// Stable local principal key.
    pub principal: PrincipalKey,
    /// Complete accepted kernel credentials.
    pub peer: PeerCredentials,
    /// Current worker epoch.
    pub worker_epoch: ConnectionIdentifier,
    /// Server-generated connection identifier.
    pub connection_id: ConnectionIdentifier,
    /// Client request identifier.
    pub request_id: RequestIdentifier,
}

/// Bounded server-core execution plan returned by a semantic handler.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchPlan {
    /// Operation is unavailable in this worker slice.
    Unsupported,
    /// Deterministic unary fixture.
    #[cfg(feature = "worker-test-fixtures")]
    MockUnary {
        /// Delay before completion.
        delay_ms: u64,
    },
    /// Deterministic streaming fixture.
    #[cfg(feature = "worker-test-fixtures")]
    MockStream {
        /// Number of bounded items.
        items: u32,
        /// Delay between items.
        interval_ms: u64,
    },
}

/// Typed semantic boundary; implementations receive no socket or raw JSON.
pub trait SemanticDispatcher: Send + Sync + std::fmt::Debug {
    /// Produces one bounded execution plan.
    fn dispatch<'a>(
        &'a self,
        context: &'a DispatchContext,
        request: &'a SemanticRequest,
    ) -> Pin<Box<dyn Future<Output = DispatchPlan> + Send + 'a>>;
}

/// Production placeholder until discovery handlers land in issue #260.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedDispatcher;

impl SemanticDispatcher for UnsupportedDispatcher {
    fn dispatch<'a>(
        &'a self,
        _context: &'a DispatchContext,
        _request: &'a SemanticRequest,
    ) -> Pin<Box<dyn Future<Output = DispatchPlan> + Send + 'a>> {
        Box::pin(async { DispatchPlan::Unsupported })
    }
}

/// Deterministic dispatcher available only to server-core tests.
#[cfg(feature = "worker-test-fixtures")]
#[derive(Debug, Clone, Copy, Default)]
pub struct MockDispatcher;

#[cfg(feature = "worker-test-fixtures")]
impl SemanticDispatcher for MockDispatcher {
    fn dispatch<'a>(
        &'a self,
        _context: &'a DispatchContext,
        request: &'a SemanticRequest,
    ) -> Pin<Box<dyn Future<Output = DispatchPlan> + Send + 'a>> {
        Box::pin(async move {
            match request {
                SemanticRequest::Reserved => DispatchPlan::Unsupported,
                SemanticRequest::MockUnary { delay_ms }
                | SemanticRequest::MockLifecycle { delay_ms } => DispatchPlan::MockUnary {
                    delay_ms: *delay_ms,
                },
                SemanticRequest::MockStream { items, interval_ms } => DispatchPlan::MockStream {
                    items: *items,
                    interval_ms: *interval_ms,
                },
            }
        })
    }
}
