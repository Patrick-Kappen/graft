//! Container runtime abstraction.

pub mod podman;

use anyhow::Result;

/// Status of a container.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ContainerStatus {
    /// Container is running.
    Running,
    /// Container exists but is not running.
    Stopped,
    /// Container does not exist.
    NotFound,
}

/// Abstraction over a container runtime.
pub trait ContainerRuntime {
    /// Start a container by name.
    ///
    /// # Errors
    ///
    /// Returns an error if the container cannot be started.
    fn start(&self, name: &str) -> Result<()>;

    /// Stop a container by name.
    ///
    /// # Errors
    ///
    /// Returns an error if the container cannot be stopped.
    fn stop(&self, name: &str) -> Result<()>;

    /// Execute a command interactively inside a running container.
    ///
    /// # Errors
    ///
    /// Returns an error if exec fails.
    fn exec_interactive(&self, name: &str, cmd: &[&str]) -> Result<()>;

    /// Get the status of a container.
    ///
    /// # Errors
    ///
    /// Returns an error if the status cannot be determined.
    fn status(&self, name: &str) -> Result<ContainerStatus>;
}
