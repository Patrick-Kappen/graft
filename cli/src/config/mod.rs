//! Configuration loading abstraction.

use anyhow::Result;
use serde::Deserialize;

/// A parsed container configuration.
#[derive(Debug, Clone, Deserialize)]
pub struct ContainerConfig {
    /// Container name.
    pub name: String,
}

/// Abstraction over configuration sources.
pub trait ConfigProvider {
    /// Load configuration for a named container.
    ///
    /// # Errors
    ///
    /// Returns an error if the config cannot be found or parsed.
    fn load(&self, name: &str) -> Result<ContainerConfig>;
}
