//! Workspace detection and management.

use anyhow::Result;
use std::path::PathBuf;

/// Information about the current workspace.
#[derive(Debug, Clone)]
pub struct Workspace {
    /// Root directory of the workspace (where `.graft/` lives).
    pub root: PathBuf,
}

/// Abstraction over workspace discovery.
pub trait WorkspaceProvider {
    /// Find the workspace root by walking up from the current directory.
    ///
    /// # Errors
    ///
    /// Returns an error if no `.graft/` directory is found.
    fn find(&self) -> Result<Workspace>;
}
