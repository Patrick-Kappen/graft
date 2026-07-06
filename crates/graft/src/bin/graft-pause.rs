#![deny(clippy::all)]
#![deny(clippy::pedantic)]

//! Minimal keep-alive process for Graft containers.

use std::{mem::MaybeUninit, process::ExitCode, ptr};

fn main() -> ExitCode {
    let mut signals = MaybeUninit::<libc::sigset_t>::uninit();

    // SAFETY: `signals` points to valid, writable memory for a `sigset_t`.
    if unsafe { libc::sigemptyset(signals.as_mut_ptr()) } != 0 {
        return ExitCode::FAILURE;
    }

    // SAFETY: `sigemptyset` succeeded, so `signals` now contains an initialized
    // `sigset_t`.
    let mut signals = unsafe { signals.assume_init() };

    // SAFETY: `signals` is an initialized `sigset_t`, and `SIGTERM` is a valid
    // signal number.
    if unsafe { libc::sigaddset(&mut signals, libc::SIGTERM) } != 0 {
        return ExitCode::FAILURE;
    }

    // SAFETY: `signals` is an initialized `sigset_t`, and `SIGINT` is a valid
    // signal number.
    if unsafe { libc::sigaddset(&mut signals, libc::SIGINT) } != 0 {
        return ExitCode::FAILURE;
    }

    // SAFETY: `signals` points to an initialized signal set. A null old-set
    // pointer is allowed when the previous mask is not needed.
    if unsafe { libc::pthread_sigmask(libc::SIG_BLOCK, &signals, ptr::null_mut()) } != 0 {
        return ExitCode::FAILURE;
    }

    loop {
        let mut received = 0;

        // SAFETY: `signals` points to an initialized signal set, and
        // `received` points to valid writable memory for the received signal.
        let result = unsafe { libc::sigwait(&signals, &mut received) };

        if result == 0 && matches!(received, libc::SIGTERM | libc::SIGINT) {
            return ExitCode::SUCCESS;
        }

        if result != libc::EINTR {
            return ExitCode::FAILURE;
        }
    }
}
