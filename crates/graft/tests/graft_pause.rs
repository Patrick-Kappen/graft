use std::{convert::TryFrom, process::Command, thread, time::Duration};

#[test]
fn exits_successfully_on_sigterm() {
    assert_exits_successfully_on(libc::SIGTERM);
}

#[test]
fn exits_successfully_on_sigint() {
    assert_exits_successfully_on(libc::SIGINT);
}

fn assert_exits_successfully_on(signal: libc::c_int) {
    let mut child = Command::new(env!("CARGO_BIN_EXE_graft-pause"))
        .spawn()
        .expect("graft-pause starts");

    let pid = libc::pid_t::try_from(child.id()).expect("child pid fits in pid_t");

    thread::sleep(Duration::from_millis(50));

    // SAFETY: `pid` is the PID returned by `Command::spawn`, and `signal` is
    // provided by libc constants in the tests.
    let result = unsafe { libc::kill(pid, signal) };
    assert_eq!(result, 0, "sending signal to graft-pause succeeds");

    for _ in 0..200 {
        if let Some(status) = child.try_wait().expect("child status can be read") {
            assert!(status.success(), "graft-pause exits successfully");
            return;
        }

        thread::sleep(Duration::from_millis(10));
    }

    child.kill().expect("stuck graft-pause can be killed");
    panic!("graft-pause did not exit after signal");
}
