use std::process::Command;

#[test]
fn system_publisher_exposes_only_generation_and_fixed_context_metadata() {
    let output = Command::new(env!("CARGO_BIN_EXE_graft-manifest-publish-system"))
        .arg("--help")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("<GENERATION>"));
    assert!(stdout.contains("--graft-gid <GRAFT_GID>"));
    assert!(stdout.contains("--producer-name <PRODUCER_NAME>"));
    assert!(stdout.contains("--producer-version <PRODUCER_VERSION>"));
    assert!(stdout.contains("--producer-build-id <PRODUCER_BUILD_ID>"));
    assert!(!stdout.contains("--parent"));
    assert!(!stdout.contains("--lock"));
    assert!(!stdout.contains("--current"));
}

#[test]
fn user_publisher_exposes_only_the_relaxed_base_directory_escape_hatch() {
    let output = Command::new(env!("CARGO_BIN_EXE_graft-manifest-publish-user"))
        .arg("--help")
        .output()
        .unwrap();

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("--relaxed-base-dirs"));
    assert!(!stdout.contains("--parent"));
    assert!(!stdout.contains("--runtime"));
}

#[test]
fn system_publisher_rejects_invalid_producer_before_publication() {
    let output = Command::new(env!("CARGO_BIN_EXE_graft-manifest-publish-system"))
        .args([
            "/nix/store/not-a-generation",
            "--graft-gid",
            "0",
            "--producer-name",
            "graft/invalid",
            "--producer-version",
            env!("CARGO_PKG_VERSION"),
            "--producer-build-id",
            "source",
        ])
        .output()
        .unwrap();

    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8(output.stderr)
        .unwrap()
        .contains("invalid installed producer identity"));
}
