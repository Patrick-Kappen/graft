use std::{
    collections::BTreeSet,
    fs,
    path::{Path, PathBuf},
    process::{Command, Output, Stdio},
};

use graft::manifest::{EndpointDescriptor, Manifest, MAX_MANIFEST_BYTES};
use serde_json::{json, Value};
use tempfile::TempDir;

const HOST_ID: &str = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20";
const MANIFEST_DIGEST: &str = "b882ca6c390d9e76498ede44b6075d92a8bee61c09db8bbbcb8b7fadcdbfed9d";
const ENDPOINT_DIGEST: &str = "ebaaca0727249bd68ea4f5567bfc80eab77dc5e999aabbc44936babdcd341f98";

fn preimage() -> Value {
    json!({
        "producer":{"name":"graft","version":"0.3.0-alpha.1","buildId":"graft-test-build"},
        "hostId":HOST_ID,
        "target":"system",
        "manager":"system",
        "workerApiRange":{"major":1,"min_minor":0,"max_minor":0},
        "workloads":[]
    })
}

fn write_preimage(directory: &TempDir, value: &Value) -> PathBuf {
    let path = directory.path().join("preimage.json");
    fs::write(&path, serde_json::to_vec(value).unwrap()).unwrap();
    path
}

fn run(arguments: &[&Path]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_graft-manifest-render"))
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .expect("manifest renderer process can start")
}

#[test]
fn helper_reads_one_file_and_writes_the_exact_canonical_pair() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_preimage(&directory, &preimage());
    let output = directory.path().join("rendered");

    let result = run(&[&input, &output]);

    assert!(result.status.success(), "renderer failed: {result:?}");
    assert!(result.stdout.is_empty());
    assert!(result.stderr.is_empty());
    let entries = fs::read_dir(&output)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        entries,
        BTreeSet::from(["endpoint.json".to_owned(), "manifest.json".to_owned()])
    );

    let manifest_bytes = fs::read(output.join("manifest.json")).unwrap();
    let endpoint_bytes = fs::read(output.join("endpoint.json")).unwrap();
    let manifest = Manifest::from_json(&manifest_bytes).unwrap();
    let endpoint = EndpointDescriptor::from_json(&endpoint_bytes).unwrap();
    assert_eq!(manifest.digest().as_str(), MANIFEST_DIGEST);
    assert_eq!(endpoint.endpoint_digest().as_str(), ENDPOINT_DIGEST);
    assert_eq!(endpoint.generation_id(), manifest.generation_id());
    assert_eq!(endpoint.manifest_digest(), manifest.digest());
}

#[test]
fn helper_rejects_invalid_preimages_before_creating_output() {
    let directory = tempfile::tempdir().unwrap();
    let mut unknown = preimage();
    unknown["token"] = "do-not-print-this-secret".into();
    let mut mismatch = preimage();
    mismatch["manager"] = "user".into();

    for (index, value) in [unknown, mismatch].iter().enumerate() {
        let input = directory.path().join(format!("invalid-{index}.json"));
        fs::write(&input, serde_json::to_vec(value).unwrap()).unwrap();
        let output = directory.path().join(format!("output-{index}"));
        let result = run(&[&input, &output]);

        assert!(!result.status.success());
        assert!(result.stdout.is_empty());
        assert!(!output.exists(), "invalid input must not produce output");
        assert!(!String::from_utf8_lossy(&result.stderr).contains("do-not-print-this-secret"));
    }
}

#[test]
fn helper_rejects_oversized_input_and_an_existing_output() {
    let directory = tempfile::tempdir().unwrap();
    let oversized = directory.path().join("oversized.json");
    fs::write(
        &oversized,
        vec![b' '; usize::try_from(MAX_MANIFEST_BYTES).unwrap() + 1],
    )
    .unwrap();
    let oversized_output = directory.path().join("oversized-output");
    let oversized_result = run(&[&oversized, &oversized_output]);
    assert!(!oversized_result.status.success());
    assert!(!oversized_output.exists());

    let input = write_preimage(&directory, &preimage());
    let existing = directory.path().join("existing");
    fs::create_dir(&existing).unwrap();
    fs::write(existing.join("sentinel"), "retain").unwrap();
    let existing_result = run(&[&input, &existing]);
    assert!(!existing_result.status.success());
    assert_eq!(
        fs::read_to_string(existing.join("sentinel")).unwrap(),
        "retain"
    );
    assert!(!existing.join("manifest.json").exists());
    assert!(!existing.join("endpoint.json").exists());
}

#[test]
fn helper_never_treats_stdin_as_the_preimage() {
    let directory = tempfile::tempdir().unwrap();
    let input = write_preimage(&directory, &preimage());
    let output = directory.path().join("rendered");
    let result = Command::new(env!("CARGO_BIN_EXE_graft-manifest-render"))
        .arg(&output)
        .stdin(fs::File::open(input).unwrap())
        .output()
        .unwrap();

    assert!(!result.status.success());
    assert!(result.stdout.is_empty());
    assert!(!output.exists());
}
