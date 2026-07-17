//! Bounded socket-activated local worker server core.

pub mod activation;
mod clock;
mod dispatcher;
mod framing;
mod limits;
pub mod protocol;
mod server;

#[cfg(feature = "worker-test-fixtures")]
pub use dispatcher::MockDispatcher;
pub use dispatcher::{
    DispatchContext, DispatchPlan, PeerCredentials, PrincipalKey, SemanticDispatcher,
    UnsupportedDispatcher,
};
pub use server::{serve, ServerConfig, ServerError};
