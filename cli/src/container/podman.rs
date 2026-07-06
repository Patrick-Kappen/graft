//! Podman implementation of [`ContainerRuntime`].

use anyhow::{bail, Context, Result};
use std::process::Command;
use tracing::debug;

use super::{ContainerRuntime, ContainerStatus};

/// Podman-backed container runtime.
///
/// Invokes `podman` and `systemctl --user` as subprocesses.
pub struct Podman;

impl Podman {
    /// Create a new [`Podman`] runtime.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl Default for Podman {
    fn default() -> Self {
        Self::new()
    }
}

impl ContainerRuntime for Podman {
    fn start(&self, name: &str) -> Result<()> {
        debug!("starting container via systemctl: {name}");
        let output = Command::new("systemctl")
            .args(["--user", "start", &format!("{name}.service")])
            .output()
            .context("failed to invoke systemctl")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("systemctl start {name} failed: {stderr}");
        }
        Ok(())
    }

    fn stop(&self, name: &str) -> Result<()> {
        debug!("stopping container via systemctl: {name}");
        let output = Command::new("systemctl")
            .args(["--user", "stop", &format!("{name}.service")])
            .output()
            .context("failed to invoke systemctl")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("systemctl stop {name} failed: {stderr}");
        }
        Ok(())
    }

    fn exec_interactive(&self, name: &str, cmd: &[&str]) -> Result<()> {
        debug!("exec in container {name}: {cmd:?}");
        let status = Command::new("podman")
            .arg("exec")
            .arg("-it")
            .arg(name)
            .args(cmd)
            .status()
            .context("failed to invoke podman exec")?;

        if !status.success() {
            bail!("podman exec exited with {status}");
        }
        Ok(())
    }

    fn status(&self, name: &str) -> Result<ContainerStatus> {
        debug!("checking container status: {name}");
        let output = Command::new("podman")
            .args(["inspect", "--format", "{{.State.Status}}", name])
            .output()
            .context("failed to invoke podman inspect")?;

        if !output.status.success() {
            return Ok(ContainerStatus::NotFound);
        }

        let state = String::from_utf8_lossy(&output.stdout)
            .trim()
            .to_string();

        match state.as_str() {
            "running" => Ok(ContainerStatus::Running),
            _ => Ok(ContainerStatus::Stopped),
        }
    }
}
