//! Shared worker/principal admission accounting.

use std::collections::BTreeMap;
use std::sync::{Arc, Mutex};

/// Fixed worker-core resource budgets.
#[derive(Debug, Clone, Copy)]
pub struct WorkerLimits;

impl WorkerLimits {
    /// Connections per principal.
    pub const CONNECTIONS_PER_PRINCIPAL: usize = 16;
    /// Connections worker-wide.
    pub const CONNECTIONS_WORKER: usize = 128;
    /// Incomplete handshakes per principal.
    pub const HANDSHAKES_PER_PRINCIPAL: usize = 4;
    /// Incomplete handshakes worker-wide.
    pub const HANDSHAKES_WORKER: usize = 32;
    /// In-flight requests per principal.
    pub const REQUESTS_PER_PRINCIPAL: usize = 64;
    /// In-flight requests worker-wide.
    pub const REQUESTS_WORKER: usize = 256;
    /// Active streams per principal.
    pub const STREAMS_PER_PRINCIPAL: usize = 16;
    /// Active streams worker-wide.
    pub const STREAMS_WORKER: usize = 64;
    /// Buffered response bytes per principal.
    pub const BUFFERED_BYTES_PER_PRINCIPAL: usize = 2 * 1024 * 1024;
    /// Buffered response bytes worker-wide.
    pub const BUFFERED_BYTES_WORKER: usize = 16 * 1024 * 1024;
}

/// Shared bounded admission registry keyed by peer UID.
#[derive(Debug, Clone, Default)]
pub struct AdmissionRegistry {
    state: Arc<Mutex<AdmissionState>>,
}

#[derive(Debug, Default)]
struct AdmissionState {
    totals: Usage,
    principals: BTreeMap<u32, Usage>,
}

#[derive(Debug, Clone, Copy, Default)]
struct Usage {
    connections: usize,
    handshakes: usize,
    requests: usize,
    streams: usize,
    buffered_bytes: usize,
}

#[derive(Debug, Clone, Copy)]
enum Resource {
    Connection,
    Handshake,
    Request,
    #[cfg_attr(not(feature = "worker-test-fixtures"), allow(dead_code))]
    Stream,
    BufferedBytes(usize),
}

impl AdmissionRegistry {
    /// Reserves one connection slot.
    #[must_use]
    pub fn connection(&self, uid: u32) -> Option<AdmissionPermit> {
        self.reserve(uid, Resource::Connection)
    }

    /// Reserves one incomplete-handshake slot.
    #[must_use]
    pub fn handshake(&self, uid: u32) -> Option<AdmissionPermit> {
        self.reserve(uid, Resource::Handshake)
    }

    /// Reserves one in-flight-request slot.
    #[must_use]
    pub fn request(&self, uid: u32) -> Option<AdmissionPermit> {
        self.reserve(uid, Resource::Request)
    }

    /// Reserves one active-stream slot.
    #[must_use]
    #[cfg_attr(not(feature = "worker-test-fixtures"), allow(dead_code))]
    pub fn stream(&self, uid: u32) -> Option<AdmissionPermit> {
        self.reserve(uid, Resource::Stream)
    }

    /// Reserves encoded response bytes.
    #[must_use]
    pub fn buffered_bytes(&self, uid: u32, bytes: usize) -> Option<AdmissionPermit> {
        self.reserve(uid, Resource::BufferedBytes(bytes))
    }

    fn reserve(&self, uid: u32, resource: Resource) -> Option<AdmissionPermit> {
        let mut state = self.state.lock().ok()?;
        let principal = state.principals.get(&uid).copied().unwrap_or_default();
        if !fits(state.totals, principal, resource) {
            return None;
        }
        increment(&mut state.totals, resource);
        increment(state.principals.entry(uid).or_default(), resource);
        Some(AdmissionPermit {
            registry: self.clone(),
            uid,
            resource,
        })
    }

    fn release(&self, uid: u32, resource: Resource) {
        if let Ok(mut state) = self.state.lock() {
            decrement(&mut state.totals, resource);
            if let Some(principal) = state.principals.get_mut(&uid) {
                decrement(principal, resource);
                if is_empty(*principal) {
                    state.principals.remove(&uid);
                }
            }
        }
    }
}

/// Per-connection negotiated buffered-byte accounting.
#[derive(Debug, Clone)]
pub struct ConnectionBuffer {
    state: Arc<Mutex<usize>>,
    maximum: usize,
}

impl ConnectionBuffer {
    /// Creates a negotiated byte budget.
    #[must_use]
    pub fn new(maximum: usize) -> Self {
        Self {
            state: Arc::new(Mutex::new(0)),
            maximum,
        }
    }

    /// Reserves bytes before queueing a response.
    #[must_use]
    pub fn reserve(&self, bytes: usize) -> Option<ConnectionBufferPermit> {
        let mut used = self.state.lock().ok()?;
        if used.saturating_add(bytes) > self.maximum {
            return None;
        }
        *used += bytes;
        Some(ConnectionBufferPermit {
            budget: self.clone(),
            bytes,
        })
    }
}

/// RAII per-connection byte reservation.
#[derive(Debug)]
pub struct ConnectionBufferPermit {
    budget: ConnectionBuffer,
    bytes: usize,
}

impl Drop for ConnectionBufferPermit {
    fn drop(&mut self) {
        if let Ok(mut used) = self.budget.state.lock() {
            *used = used.saturating_sub(self.bytes);
        }
    }
}

/// RAII admission reservation.
#[derive(Debug)]
pub struct AdmissionPermit {
    registry: AdmissionRegistry,
    uid: u32,
    resource: Resource,
}

impl Drop for AdmissionPermit {
    fn drop(&mut self) {
        self.registry.release(self.uid, self.resource);
    }
}

fn fits(total: Usage, principal: Usage, resource: Resource) -> bool {
    match resource {
        Resource::Connection => {
            principal.connections < WorkerLimits::CONNECTIONS_PER_PRINCIPAL
                && total.connections < WorkerLimits::CONNECTIONS_WORKER
        }
        Resource::Handshake => {
            principal.handshakes < WorkerLimits::HANDSHAKES_PER_PRINCIPAL
                && total.handshakes < WorkerLimits::HANDSHAKES_WORKER
        }
        Resource::Request => {
            principal.requests < WorkerLimits::REQUESTS_PER_PRINCIPAL
                && total.requests < WorkerLimits::REQUESTS_WORKER
        }
        Resource::Stream => {
            principal.streams < WorkerLimits::STREAMS_PER_PRINCIPAL
                && total.streams < WorkerLimits::STREAMS_WORKER
        }
        Resource::BufferedBytes(bytes) => {
            principal.buffered_bytes.saturating_add(bytes)
                <= WorkerLimits::BUFFERED_BYTES_PER_PRINCIPAL
                && total.buffered_bytes.saturating_add(bytes) <= WorkerLimits::BUFFERED_BYTES_WORKER
        }
    }
}

fn increment(usage: &mut Usage, resource: Resource) {
    match resource {
        Resource::Connection => usage.connections += 1,
        Resource::Handshake => usage.handshakes += 1,
        Resource::Request => usage.requests += 1,
        Resource::Stream => usage.streams += 1,
        Resource::BufferedBytes(bytes) => usage.buffered_bytes += bytes,
    }
}

fn decrement(usage: &mut Usage, resource: Resource) {
    match resource {
        Resource::Connection => usage.connections = usage.connections.saturating_sub(1),
        Resource::Handshake => usage.handshakes = usage.handshakes.saturating_sub(1),
        Resource::Request => usage.requests = usage.requests.saturating_sub(1),
        Resource::Stream => usage.streams = usage.streams.saturating_sub(1),
        Resource::BufferedBytes(bytes) => {
            usage.buffered_bytes = usage.buffered_bytes.saturating_sub(bytes);
        }
    }
}

const fn is_empty(usage: Usage) -> bool {
    usage.connections == 0
        && usage.handshakes == 0
        && usage.requests == 0
        && usage.streams == 0
        && usage.buffered_bytes == 0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn principal_connection_limit_releases_with_permits() {
        let registry = AdmissionRegistry::default();
        let permits: Vec<_> = (0..WorkerLimits::CONNECTIONS_PER_PRINCIPAL)
            .map(|_| registry.connection(1000).unwrap())
            .collect();
        assert!(registry.connection(1000).is_none());
        assert!(registry.connection(1001).is_some());

        drop(permits);

        assert!(registry.connection(1000).is_some());
    }

    #[test]
    fn worker_connection_and_handshake_limits_apply_across_principals() {
        let registry = AdmissionRegistry::default();
        let connections: Vec<_> = (0..WorkerLimits::CONNECTIONS_WORKER)
            .map(|uid| registry.connection(u32::try_from(uid).unwrap()).unwrap())
            .collect();
        assert!(registry.connection(10_000).is_none());

        drop(connections);
        let handshakes: Vec<_> = (0..WorkerLimits::HANDSHAKES_WORKER)
            .map(|uid| registry.handshake(u32::try_from(uid).unwrap()).unwrap())
            .collect();
        assert!(registry.handshake(10_000).is_none());
        drop(handshakes);
    }

    #[test]
    fn request_limits_apply_per_principal_and_worker_and_release() {
        let registry = AdmissionRegistry::default();
        let principal: Vec<_> = (0..WorkerLimits::REQUESTS_PER_PRINCIPAL)
            .map(|_| registry.request(1000).unwrap())
            .collect();
        assert!(registry.request(1000).is_none());
        drop(principal);
        assert!(registry.request(1000).is_some());

        let worker: Vec<_> = (0..WorkerLimits::REQUESTS_WORKER)
            .map(|index| registry.request(u32::try_from(index).unwrap()).unwrap())
            .collect();
        assert!(registry.request(10_000).is_none());
        drop(worker);
        assert!(registry.request(10_000).is_some());
    }

    #[test]
    fn stream_limits_apply_per_principal_and_worker_and_release() {
        let registry = AdmissionRegistry::default();
        let principal: Vec<_> = (0..WorkerLimits::STREAMS_PER_PRINCIPAL)
            .map(|_| registry.stream(1000).unwrap())
            .collect();
        assert!(registry.stream(1000).is_none());
        drop(principal);
        assert!(registry.stream(1000).is_some());

        let worker: Vec<_> = (0..WorkerLimits::STREAMS_WORKER)
            .map(|index| registry.stream(u32::try_from(index).unwrap()).unwrap())
            .collect();
        assert!(registry.stream(10_000).is_none());
        drop(worker);
        assert!(registry.stream(10_000).is_some());
    }

    #[test]
    fn buffered_byte_limit_is_checked_before_reservation() {
        let connection = ConnectionBuffer::new(10);
        let connection_permit = connection.reserve(10).unwrap();
        assert!(connection.reserve(1).is_none());
        drop(connection_permit);
        assert!(connection.reserve(1).is_some());

        let registry = AdmissionRegistry::default();
        let permit = registry
            .buffered_bytes(1000, WorkerLimits::BUFFERED_BYTES_PER_PRINCIPAL)
            .unwrap();
        assert!(registry.buffered_bytes(1000, 1).is_none());
        drop(permit);
        assert!(registry.buffered_bytes(1000, 1).is_some());
    }
}
