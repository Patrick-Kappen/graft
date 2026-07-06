#![deny(clippy::all)]
#![deny(clippy::pedantic)]

//! Minimal keep-alive process for Graft containers.

fn main() {
    loop {
        // SAFETY: `pause(2)` has no preconditions. It blocks until a signal is
        // delivered and then returns. Continuing the loop keeps the process
        // alive after handled signals.
        let _ = unsafe { libc::pause() };
    }
}
