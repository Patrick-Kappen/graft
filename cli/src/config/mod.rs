//! Configuration loading abstraction.

pub mod schema;
pub mod toml_config;

pub use schema::ContainerConfig;

use anyhow::Result;

/// Abstraction over configuration sources.
pub trait ConfigProvider {
    /// Load configuration for a named container.
    ///
    /// # Errors
    ///
    /// Returns an error if the config cannot be found or parsed.
    fn load(&self, name: &str) -> Result<ContainerConfig>;
}
