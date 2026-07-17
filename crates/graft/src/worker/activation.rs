//! Strict systemd socket-activation boundary.

use std::env;
use std::os::fd::{BorrowedFd, FromRawFd as _};
use std::os::unix::net::UnixListener;

use rustix::net::{AddressFamily, SocketType};
use thiserror::Error;

const ACTIVATION_FD: i32 = 3;
const DESCRIPTOR_NAME: &str = "graft-worker";

/// Socket-activation validation failure.
#[derive(Debug, Error)]
pub enum ActivationError {
    /// A required environment value is absent or malformed.
    #[error("invalid systemd socket activation environment")]
    Environment,
    /// Activation belongs to another process.
    #[error("socket activation PID does not match this process")]
    WrongProcess,
    /// Exactly one descriptor was not supplied.
    #[error("exactly one activated descriptor is required")]
    DescriptorCount,
    /// Descriptor name does not match fixed worker policy.
    #[error("activated descriptor name is invalid")]
    DescriptorName,
    /// Descriptor is not one listening Unix stream socket.
    #[error("activated descriptor is not a listening Unix stream socket")]
    DescriptorType,
    /// Descriptor validation failed.
    #[error("failed to validate activated descriptor")]
    Io(#[source] std::io::Error),
}

/// Takes ownership of exactly one validated inherited listener.
///
/// # Errors
///
/// Returns an error for missing, malformed, foreign, multiple, wrongly named,
/// non-Unix, non-stream, or non-listening activation descriptors.
pub fn take_listener() -> Result<UnixListener, ActivationError> {
    let listen_pid = parse_environment_u32("LISTEN_PID")?;
    if listen_pid != std::process::id() {
        return Err(ActivationError::WrongProcess);
    }
    if parse_environment_u32("LISTEN_FDS")? != 1 {
        return Err(ActivationError::DescriptorCount);
    }
    if env::var("LISTEN_FDNAMES").map_err(|_| ActivationError::Environment)? != DESCRIPTOR_NAME {
        return Err(ActivationError::DescriptorName);
    }

    // SAFETY: `fcntl(F_GETFD)` accepts an arbitrary integer descriptor and
    // reports `EBADF` without requiring an I/O-safe borrowed descriptor.
    if unsafe { libc::fcntl(ACTIVATION_FD, libc::F_GETFD) } == -1 {
        return Err(ActivationError::Io(std::io::Error::last_os_error()));
    }
    // SAFETY: the successful raw `F_GETFD` probe above established that fd 3 is
    // open. The borrowed view does not outlive this call and ownership is
    // transferred exactly once below.
    let descriptor = unsafe { BorrowedFd::borrow_raw(ACTIVATION_FD) };
    if rustix::net::sockopt::socket_domain(descriptor).map_err(errno_to_io)? != AddressFamily::UNIX
        || rustix::net::sockopt::socket_type(descriptor).map_err(errno_to_io)? != SocketType::STREAM
        || !rustix::net::sockopt::socket_acceptconn(descriptor).map_err(errno_to_io)?
    {
        return Err(ActivationError::DescriptorType);
    }

    clear_environment();
    // SAFETY: the validated activation descriptor is uniquely transferred into
    // the listener and is not used again through the borrowed descriptor.
    Ok(unsafe { UnixListener::from_raw_fd(ACTIVATION_FD) })
}

fn parse_environment_u32(name: &str) -> Result<u32, ActivationError> {
    env::var(name)
        .map_err(|_| ActivationError::Environment)?
        .parse()
        .map_err(|_| ActivationError::Environment)
}

fn clear_environment() {
    env::remove_var("LISTEN_PID");
    env::remove_var("LISTEN_FDS");
    env::remove_var("LISTEN_FDNAMES");
}

fn errno_to_io(error: rustix::io::Errno) -> ActivationError {
    ActivationError::Io(std::io::Error::from_raw_os_error(error.raw_os_error()))
}
