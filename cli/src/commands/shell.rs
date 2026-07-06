//! `graft shell <name>` — open an interactive shell in a container.

use anyhow::Result;
use tracing::info;

use crate::container::{ContainerRuntime, ContainerStatus};

/// Run `graft shell <name>`.
///
/// Starts the container if not already running, then opens an interactive shell
/// using the value of `$SHELL`, falling back to `/bin/sh`.
///
/// # Errors
///
/// Returns an error if the container cannot be started or exec fails.
pub fn run(runtime: &dyn ContainerRuntime, name: &str) -> Result<()> {
    match runtime.status(name)? {
        ContainerStatus::Running => {
            info!("container '{name}' is already running");
        }
        ContainerStatus::Stopped | ContainerStatus::NotFound => {
            info!("starting container '{name}'");
            runtime.start(name)?;
        }
    }

    let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".to_string());
    runtime.exec_interactive(name, &[&shell])
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use std::cell::Cell;

    struct MockRuntime {
        status: ContainerStatus,
        started: Cell<bool>,
        execed: Cell<bool>,
    }

    impl MockRuntime {
        fn new(status: ContainerStatus) -> Self {
            Self {
                status,
                started: Cell::new(false),
                execed: Cell::new(false),
            }
        }
    }

    impl ContainerRuntime for MockRuntime {
        fn start(&self, _name: &str) -> Result<()> {
            self.started.set(true);
            Ok(())
        }

        fn stop(&self, _name: &str) -> Result<()> {
            Ok(())
        }

        fn exec_interactive(&self, _name: &str, _cmd: &[&str]) -> Result<()> {
            self.execed.set(true);
            Ok(())
        }

        fn status(&self, _name: &str) -> Result<ContainerStatus> {
            Ok(self.status.clone())
        }
    }

    struct FailingRuntime;

    impl ContainerRuntime for FailingRuntime {
        fn start(&self, _name: &str) -> Result<()> {
            anyhow::bail!("start failed")
        }

        fn stop(&self, _name: &str) -> Result<()> {
            Ok(())
        }

        fn exec_interactive(&self, _name: &str, _cmd: &[&str]) -> Result<()> {
            Ok(())
        }

        fn status(&self, _name: &str) -> Result<ContainerStatus> {
            Ok(ContainerStatus::NotFound)
        }
    }

    #[test]
    fn starts_and_execs_when_not_found() {
        let runtime = MockRuntime::new(ContainerStatus::NotFound);
        run(&runtime, "devshell").unwrap();
        assert!(runtime.started.get(), "should start the container");
        assert!(runtime.execed.get(), "should exec into the container");
    }

    #[test]
    fn starts_and_execs_when_stopped() {
        let runtime = MockRuntime::new(ContainerStatus::Stopped);
        run(&runtime, "devshell").unwrap();
        assert!(runtime.started.get(), "should start a stopped container");
        assert!(runtime.execed.get(), "should exec into the container");
    }

    #[test]
    fn skips_start_when_already_running() {
        let runtime = MockRuntime::new(ContainerStatus::Running);
        run(&runtime, "devshell").unwrap();
        assert!(!runtime.started.get(), "should not restart a running container");
        assert!(runtime.execed.get(), "should exec into the container");
    }

    #[test]
    fn propagates_start_error() {
        let runtime = FailingRuntime;
        let result = run(&runtime, "devshell");
        assert!(result.is_err(), "should propagate start errors");
    }
}
