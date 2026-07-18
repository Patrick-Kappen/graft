//! Bounded socket-activated local worker server core.

pub mod activation;
mod clock;
mod discovery;
mod dispatcher;
mod framing;
pub mod interlock;
pub mod lifecycle;
mod limits;
pub mod mutation;
pub mod observation;
pub mod protocol;
mod server;

pub use discovery::{
    BackendSelector, DiscoveryDispatcher, ManagerStatusAdapter, RuntimeStatusAdapter,
    UnavailableManagerAdapter, UnavailableRuntimeAdapter,
};
#[cfg(feature = "worker-test-fixtures")]
pub use discovery::{MockManagerAdapter, MockRuntimeAdapter};
#[cfg(feature = "worker-test-fixtures")]
pub use dispatcher::MockDispatcher;
pub use dispatcher::{
    DispatchContext, DispatchPlan, PeerCredentials, PrincipalKey, SemanticDispatcher,
    UnsupportedDispatcher,
};
pub use server::{serve, ServerConfig, ServerError};
