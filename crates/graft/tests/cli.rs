use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};

use serde_json::Value;
use tempfile::TempDir;

const MINIMAL_CONFIG: &str = r#"
version = 1
name = "worker"

[deploy]
target = "system"
"#;

fn write_config(directory: &TempDir, file_name: &str, content: &str) -> PathBuf {
    let path = directory.path().join(file_name);
    fs::write(&path, content).expect("test config can be written");
    path
}

fn run_graft(path: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_graft"))
        .arg(path)
        .env_remove("RUST_LOG")
        .output()
        .expect("graft process can be started")
}

fn parse_successful_json(output: &Output) -> Value {
    assert!(output.status.success(), "graft should succeed: {output:?}");
    assert!(
        output.stderr.is_empty(),
        "successful stderr should be empty"
    );
    serde_json::from_slice(&output.stdout).expect("successful stdout should be JSON")
}

fn assert_failed_without_stdout(output: &Output, expected_stderr: &[&str]) {
    assert!(!output.status.success(), "graft should fail");
    assert!(output.stdout.is_empty(), "failed stdout should be empty");

    let stderr = String::from_utf8_lossy(&output.stderr);
    for expected in expected_stderr {
        assert!(
            stderr.contains(expected),
            "stderr should contain {expected:?}, got {stderr:?}"
        );
    }
}

#[test]
fn minimal_config_writes_resolved_json_with_one_trailing_newline() {
    let directory = tempfile::tempdir().expect("temporary directory can be created");
    let config = write_config(&directory, "worker.toml", MINIMAL_CONFIG);

    let output = run_graft(&config);

    let resolved = parse_successful_json(&output);
    assert_eq!(resolved["name"], "worker");
    assert_eq!(resolved["deploy"]["target"], "system");
    assert_eq!(resolved["filesystem"]["readOnly"], true);
    assert_eq!(resolved["security"]["dropCapabilities"][0], "all");
    assert_eq!(resolved["security"]["noNewPrivileges"], true);
    assert!(output.stdout.ends_with(b"\n"));
    assert!(!output.stdout.ends_with(b"\n\n"));
}

#[test]
fn explicit_supported_fields_survive_the_process_boundary() {
    let directory = tempfile::tempdir().expect("temporary directory can be created");
    let config = write_config(
        &directory,
        "explicit.toml",
        r#"
version = 1
name = "explicit"

[deploy]
target = "user"

[config.runtime]
command = ["/bin/sh", "-c", "printf ready"]

[config.container]
hostname = "worker.local"

[config.container.environment]
LOG_LEVEL = "debug"

[config.service]
restart = "on-failure"
restartSec = "5s"
"#,
    );

    let output = run_graft(&config);

    let resolved = parse_successful_json(&output);
    assert_eq!(resolved["deploy"]["target"], "user");
    assert_eq!(resolved["runtime"]["command"][0], "/bin/sh");
    assert_eq!(resolved["runtime"]["command"][2], "printf ready");
    assert_eq!(resolved["container"]["hostname"], "worker.local");
    assert_eq!(resolved["container"]["environment"]["LOG_LEVEL"], "debug");
    assert_eq!(resolved["service"]["restart"], "on-failure");
    assert_eq!(resolved["service"]["restartSec"], "5s");
}

#[test]
fn missing_file_returns_read_error_with_path_context() {
    let directory = tempfile::tempdir().expect("temporary directory can be created");
    let missing = directory.path().join("missing.toml");

    let output = run_graft(&missing);

    assert_failed_without_stdout(
        &output,
        &["cannot read config", missing.to_string_lossy().as_ref()],
    );
}

#[test]
fn directory_input_returns_read_error_with_path_context() {
    let directory = tempfile::tempdir().expect("temporary directory can be created");

    let output = run_graft(directory.path());

    assert_failed_without_stdout(
        &output,
        &[
            "cannot read config",
            directory.path().to_string_lossy().as_ref(),
        ],
    );
}

#[test]
fn malformed_toml_returns_parser_error_with_path_context() {
    let directory = tempfile::tempdir().expect("temporary directory can be created");
    let config = write_config(&directory, "malformed.toml", "version = [\n");

    let output = run_graft(&config);

    assert_failed_without_stdout(
        &output,
        &[
            "failed to parse config",
            config.to_string_lossy().as_ref(),
            "TOML parse error",
        ],
    );
}

#[test]
fn semantic_validation_error_includes_path_and_resolver_cause() {
    let directory = tempfile::tempdir().expect("temporary directory can be created");
    let config = write_config(
        &directory,
        "semantic.toml",
        "version = 1\nname = \"semantic\"\n",
    );

    let output = run_graft(&config);

    assert_failed_without_stdout(
        &output,
        &[
            config.to_string_lossy().as_ref(),
            "deploy.target is required",
        ],
    );
}

#[test]
fn unknown_field_fails_closed_without_stdout() {
    let directory = tempfile::tempdir().expect("temporary directory can be created");
    let config = write_config(
        &directory,
        "unknown.toml",
        &format!("{MINIMAL_CONFIG}\nunexpected = true\n"),
    );

    let output = run_graft(&config);

    assert_failed_without_stdout(
        &output,
        &[
            "failed to parse config",
            config.to_string_lossy().as_ref(),
            "unknown field",
        ],
    );
}

#[test]
fn trace_logging_does_not_pollute_successful_output() {
    let directory = tempfile::tempdir().expect("temporary directory can be created");
    let config = write_config(&directory, "trace.toml", MINIMAL_CONFIG);

    let output = Command::new(env!("CARGO_BIN_EXE_graft"))
        .arg(&config)
        .env("RUST_LOG", "trace")
        .output()
        .expect("graft process can be started");

    let resolved = parse_successful_json(&output);
    assert_eq!(resolved["name"], "worker");
}

#[cfg(target_os = "linux")]
#[test]
fn unwritable_stdout_returns_write_error() {
    let directory = tempfile::tempdir().expect("temporary directory can be created");
    let config = write_config(&directory, "output-error.toml", MINIMAL_CONFIG);
    let full = fs::File::options()
        .write(true)
        .open("/dev/full")
        .expect("Linux /dev/full can be opened");

    let output = Command::new(env!("CARGO_BIN_EXE_graft"))
        .arg(&config)
        .stdout(Stdio::from(full))
        .stderr(Stdio::piped())
        .output()
        .expect("graft process can be started");

    assert!(!output.status.success(), "graft should fail on /dev/full");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("failed to write resolved JSON"),
        "stderr should report the output failure, got {stderr:?}"
    );
}
