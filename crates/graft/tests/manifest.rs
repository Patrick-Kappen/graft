use graft::manifest::{EndpointDescriptor, Manifest, ManifestError};
use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

const HOST_ID: &str = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20";

fn digest(value: &Value) -> String {
    let bytes = serde_json::to_vec(value).unwrap();
    format!("{:x}", Sha256::digest(bytes))
}

fn pair(target: &str) -> (Value, Value) {
    let manager = if target == "system" { "system" } else { "user" };
    let socket = if target == "system" {
        json!({"kind":"absolute_system","value":"/run/graft/system/worker.sock"})
    } else {
        json!({"kind":"linux_user_runtime_relative","value":"graft/user/worker.sock"})
    };
    let producer = json!({
        "name": "graft",
        "version": "0.3.0-alpha.1",
        "buildId": "graft-test-build"
    });
    let api = json!({"major":1,"min_minor":0,"max_minor":0});
    let mut manifest = json!({
        "schemaVersion":{"major":1,"minor":0},
        "workerApiRange":api,
        "producer":producer,
        "hostId":HOST_ID,
        "target":target,
        "manager":manager,
        "workloadCount":0,
        "workloads":[]
    });
    let manifest_digest = digest(&manifest);
    manifest["generationId"] = manifest_digest.clone().into();
    manifest["manifestDigest"] = manifest_digest.clone().into();

    let mut endpoint = json!({
        "schemaVersion":{"major":1,"minor":0},
        "workerApiRange":api,
        "producer":producer,
        "hostId":HOST_ID,
        "target":target,
        "manager":manager,
        "generationId":manifest_digest,
        "manifestDigest":manifest_digest,
        "socketAddress":socket
    });
    endpoint["endpointDigest"] = digest(&endpoint).into();
    (manifest, endpoint)
}

fn resign_endpoint(endpoint: &mut Value) {
    endpoint.as_object_mut().unwrap().remove("endpointDigest");
    endpoint["endpointDigest"] = digest(endpoint).into();
}

fn workload(name: &str, identity: char) -> Value {
    let digest = identity.to_string().repeat(64);
    json!({
        "workloadId":digest,
        "name":name,
        "target":"system",
        "enabled":true,
        "lifecycle":"service",
        "startupIntent":"manager_target",
        "sourceIdentity":format!("{name}.toml"),
        "sourceDigest":digest,
        "resolvedDigest":digest,
        "dependencyDigest":digest,
        "quadletSourceUnit":format!("{name}.container"),
        "generatedService":format!("{name}.service"),
        "containerName":name,
        "artifactIdentity":digest,
        "rootfsStorePath":format!("/nix/store/{digest}-{name}"),
        "closureIdentity":digest,
        "dependencyServices":[],
        "lifecycleCapabilities":["up","down","restart"],
        "observabilityCapabilities":["manifest","manager","runtime"],
        "requiredWorkerApi":{"major":1,"min_minor":0,"max_minor":0},
        "requiredProducer":{
            "name":"graft",
            "version":"0.3.0-alpha.1",
            "buildId":"graft-test-build"
        },
        "requiredBackend":{
            "runtime":"podman",
            "minimumVersion":"5.0.0"
        }
    })
}

fn resign_manifest(manifest: &mut Value) {
    manifest.as_object_mut().unwrap().remove("generationId");
    manifest.as_object_mut().unwrap().remove("manifestDigest");
    let manifest_digest = digest(manifest);
    manifest["generationId"] = manifest_digest.clone().into();
    manifest["manifestDigest"] = manifest_digest.into();
}

fn set_workloads(manifest: &mut Value, workloads: Vec<Value>) {
    manifest["workloadCount"] = workloads.len().into();
    manifest["workloads"] = workloads.into();
    resign_manifest(manifest);
}

#[test]
fn canonical_digest_preimages_match_fixed_version_one_vectors() {
    let (manifest, endpoint) = pair("system");

    assert_eq!(
        manifest["manifestDigest"],
        "b882ca6c390d9e76498ede44b6075d92a8bee61c09db8bbbcb8b7fadcdbfed9d"
    );
    assert_eq!(
        endpoint["endpointDigest"],
        "ebaaca0727249bd68ea4f5567bfc80eab77dc5e999aabbc44936babdcd341f98"
    );
}

#[test]
fn valid_system_and_user_discovery_documents_load() {
    for target in ["system", "user"] {
        let (manifest, endpoint) = pair(target);

        let loaded_manifest = Manifest::from_json(&serde_json::to_vec(&manifest).unwrap());
        let loaded_endpoint =
            EndpointDescriptor::from_json(&serde_json::to_vec(&endpoint).unwrap());

        assert!(loaded_manifest.is_ok(), "valid {target} manifest must load");
        assert!(loaded_endpoint.is_ok(), "valid {target} endpoint must load");
    }
}

#[test]
fn manifest_rejects_digest_generation_unknown_field_and_build_time_uid() {
    let (mut wrong_digest, _) = pair("user");
    wrong_digest["manifestDigest"] = "0".repeat(64).into();
    let (mut wrong_generation, _) = pair("user");
    wrong_generation["generationId"] = "1".repeat(64).into();
    let (mut unknown, _) = pair("user");
    unknown["unexpected"] = true.into();
    let (mut uid, _) = pair("user");
    uid["uid"] = 1000.into();

    for invalid in [wrong_digest, wrong_generation, unknown, uid] {
        assert!(Manifest::from_json(&serde_json::to_vec(&invalid).unwrap()).is_err());
    }
}

#[test]
fn endpoint_rejects_wrong_address_digest_context_and_manifest_binding() {
    let (_, mut wrong_address) = pair("user");
    wrong_address["socketAddress"] =
        json!({"kind":"absolute_system","value":"/run/graft/system/worker.sock"});
    let (_, mut wrong_digest) = pair("system");
    wrong_digest["endpointDigest"] = "0".repeat(64).into();
    let (manifest, mut other_generation) = pair("system");
    other_generation["manifestDigest"] = "1".repeat(64).into();
    resign_endpoint(&mut other_generation);

    assert!(EndpointDescriptor::from_json(&serde_json::to_vec(&wrong_address).unwrap()).is_err());
    assert!(EndpointDescriptor::from_json(&serde_json::to_vec(&wrong_digest).unwrap()).is_err());

    let manifest = Manifest::from_json(&serde_json::to_vec(&manifest).unwrap()).unwrap();
    let endpoint = EndpointDescriptor::from_json(&serde_json::to_vec(&other_generation).unwrap());
    assert!(matches!(endpoint, Err(ManifestError::DescriptorMismatch)));
    assert_eq!(manifest.workloads().len(), 0);
}

#[test]
fn workload_records_are_typed_readable_and_require_canonical_unique_order() {
    let (mut valid, _) = pair("system");
    set_workloads(
        &mut valid,
        vec![workload("alpha", 'a'), workload("beta", 'b')],
    );
    let loaded = Manifest::from_json(&serde_json::to_vec(&valid).unwrap()).unwrap();

    assert_eq!(loaded.workload_count(), 2);
    assert_eq!(loaded.workloads()[0].name(), "alpha");
    assert_eq!(loaded.workloads()[0].generated_service(), "alpha.service");
    assert_eq!(loaded.workloads()[0].lifecycle_capabilities().len(), 3);

    let (mut reversed, _) = pair("system");
    set_workloads(
        &mut reversed,
        vec![workload("beta", 'b'), workload("alpha", 'a')],
    );
    assert!(matches!(
        Manifest::from_json(&serde_json::to_vec(&reversed).unwrap()),
        Err(ManifestError::WorkloadOrder)
    ));

    let (mut duplicate, _) = pair("system");
    set_workloads(
        &mut duplicate,
        vec![workload("alpha", 'a'), workload("alpha", 'b')],
    );
    assert!(Manifest::from_json(&serde_json::to_vec(&duplicate).unwrap()).is_err());
}

#[test]
fn manifest_rejects_context_schema_api_count_and_workload_mismatches() {
    let (mut context, _) = pair("system");
    context["manager"] = "user".into();
    resign_manifest(&mut context);
    let (mut schema, _) = pair("system");
    schema["schemaVersion"]["major"] = 2.into();
    resign_manifest(&mut schema);
    let (mut api, _) = pair("system");
    api["workerApiRange"]["major"] = 2.into();
    resign_manifest(&mut api);
    let (mut count, _) = pair("system");
    count["workloadCount"] = 1.into();
    resign_manifest(&mut count);
    let (mut workload_target, _) = pair("system");
    let mut wrong_target = workload("alpha", 'a');
    wrong_target["target"] = "user".into();
    set_workloads(&mut workload_target, vec![wrong_target]);
    let (mut store_path, _) = pair("system");
    let mut wrong_path = workload("alpha", 'a');
    wrong_path["rootfsStorePath"] = "/tmp/not-store".into();
    set_workloads(&mut store_path, vec![wrong_path]);
    let (mut dot_store_path, _) = pair("system");
    let mut dot_path = workload("alpha", 'a');
    dot_path["rootfsStorePath"] = "/nix/store/.".into();
    set_workloads(&mut dot_store_path, vec![dot_path]);
    let (mut parent_store_path, _) = pair("system");
    let mut parent_path = workload("alpha", 'a');
    parent_path["rootfsStorePath"] = "/nix/store/..".into();
    set_workloads(&mut parent_store_path, vec![parent_path]);
    let (mut bad_workload_name, _) = pair("system");
    set_workloads(&mut bad_workload_name, vec![workload("@bad", 'a')]);
    let (mut hidden_container_name, _) = pair("system");
    let mut hidden_container = workload("alpha", 'a');
    hidden_container["containerName"] = ".hidden".into();
    set_workloads(&mut hidden_container_name, vec![hidden_container]);
    let (mut unit_mismatch, _) = pair("system");
    let mut wrong_service = workload("alpha", 'a');
    wrong_service["generatedService"] = "sshd.service".into();
    set_workloads(&mut unit_mismatch, vec![wrong_service]);
    assert!(matches!(
        Manifest::from_json(&serde_json::to_vec(&unit_mismatch).unwrap()),
        Err(ManifestError::WorkloadUnitMismatch)
    ));
    let (mut unsupported_workload_api, _) = pair("system");
    unsupported_workload_api["workerApiRange"]["max_minor"] = 1.into();
    let mut future_workload = workload("alpha", 'a');
    future_workload["requiredWorkerApi"]["min_minor"] = 1.into();
    future_workload["requiredWorkerApi"]["max_minor"] = 1.into();
    set_workloads(&mut unsupported_workload_api, vec![future_workload]);

    for invalid in [
        context,
        schema,
        api,
        count,
        workload_target,
        store_path,
        dot_store_path,
        parent_store_path,
        bad_workload_name,
        hidden_container_name,
        unsupported_workload_api,
    ] {
        assert!(Manifest::from_json(&serde_json::to_vec(&invalid).unwrap()).is_err());
    }
}

#[test]
fn public_parsers_reject_oversized_input_before_json_decoding() {
    let manifest = vec![b'x'; usize::try_from(graft::manifest::MAX_MANIFEST_BYTES).unwrap() + 1];
    let endpoint = vec![b'x'; usize::try_from(graft::manifest::MAX_ENDPOINT_BYTES).unwrap() + 1];

    assert!(matches!(
        Manifest::from_json(&manifest),
        Err(ManifestError::DocumentTooLarge)
    ));
    assert!(matches!(
        EndpointDescriptor::from_json(&endpoint),
        Err(ManifestError::DocumentTooLarge)
    ));
}

#[test]
fn parser_rejects_noncanonical_json_bytes_and_duplicate_fields() {
    let (manifest, _) = pair("system");
    let mut whitespace = serde_json::to_vec(&manifest).unwrap();
    whitespace.push(b'\n');
    let duplicate = br#"{"generationId":"a","generationId":"b"}"#;

    assert!(matches!(
        Manifest::from_json(&whitespace),
        Err(ManifestError::NonCanonicalJson)
    ));
    assert!(matches!(
        Manifest::from_json(duplicate),
        Err(ManifestError::ManifestJson(_))
    ));
}

#[test]
fn identifiers_reject_noncanonical_uuid_and_digest_encodings() {
    let (mut uppercase_uuid, _) = pair("system");
    uppercase_uuid["hostId"] = HOST_ID.to_uppercase().into();
    let (mut wrong_variant, _) = pair("system");
    wrong_variant["hostId"] = "018f0f77-8c4d-7b2a-0e6a-4b8a7d3a1c20".into();
    let (mut uppercase_digest, _) = pair("system");
    uppercase_digest["manifestDigest"] = "A".repeat(64).into();

    assert!(matches!(
        Manifest::from_json(&serde_json::to_vec(&uppercase_uuid).unwrap()),
        Err(ManifestError::ManifestJson(_))
    ));
    assert!(matches!(
        Manifest::from_json(&serde_json::to_vec(&wrong_variant).unwrap()),
        Err(ManifestError::ManifestJson(_))
    ));
    assert!(matches!(
        Manifest::from_json(&serde_json::to_vec(&uppercase_digest).unwrap()),
        Err(ManifestError::ManifestJson(_))
    ));
}
