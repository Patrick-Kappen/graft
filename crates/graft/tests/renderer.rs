use graft::manifest::{render, EndpointDescriptor, Manifest, ManifestError, RenderInput};
use serde_json::{json, Value};

const HOST_ID: &str = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20";

fn input(target: &str, workloads: Vec<Value>) -> RenderInput {
    let manager = if target == "system" { "system" } else { "user" };
    serde_json::from_value(json!({
        "producer":{"name":"graft","version":"0.3.0-alpha.1","buildId":"graft-test-build"},
        "hostId":HOST_ID,
        "target":target,
        "manager":manager,
        "workerApiRange":{"major":1,"min_minor":0,"max_minor":0},
        "workloads":workloads
    }))
    .unwrap()
}

fn workload(name: &str, identity: char) -> Value {
    let digest = identity.to_string().repeat(64);
    json!({
        "workloadId":digest, "name":name, "target":"system", "enabled":true,
        "lifecycle":"long_running", "startupIntent":"manager_target",
        "sourceIdentity":format!("{name}.toml"), "sourceDigest":digest,
        "resolvedDigest":digest, "dependencyDigest":digest,
        "quadletSourceUnit":format!("{name}.container"),
        "generatedService":format!("{name}.service"), "containerName":name,
        "artifactIdentity":digest, "rootfsStorePath":format!("/nix/store/{digest}-{name}"),
        "closureIdentity":digest, "dependencyServices":[],
        "lifecycleCapabilities":["up","down","restart"],
        "observabilityCapabilities":["manifest","manager","runtime"],
        "requiredWorkerApi":{"major":1,"min_minor":0,"max_minor":0},
        "requiredProducer":{"name":"graft","version":"0.3.0-alpha.1","buildId":"graft-test-build"},
        "requiredBackend":{"runtime":"podman","minimumVersion":"5.0.0"}
    })
}

#[test]
fn renderer_emits_fixed_canonical_system_vectors_accepted_by_public_parsers() {
    let rendered = render(input("system", vec![])).unwrap();
    let manifest: Value = serde_json::from_slice(rendered.manifest_json()).unwrap();
    let endpoint: Value = serde_json::from_slice(rendered.endpoint_json()).unwrap();

    assert_eq!(
        manifest["manifestDigest"],
        "b882ca6c390d9e76498ede44b6075d92a8bee61c09db8bbbcb8b7fadcdbfed9d"
    );
    assert_eq!(
        endpoint["endpointDigest"],
        "ebaaca0727249bd68ea4f5567bfc80eab77dc5e999aabbc44936babdcd341f98"
    );
    assert_eq!(endpoint["generationId"], manifest["generationId"]);
    assert_eq!(endpoint["manifestDigest"], manifest["manifestDigest"]);
    assert!(Manifest::from_json(rendered.manifest_json()).is_ok());
    assert!(EndpointDescriptor::from_json(rendered.endpoint_json()).is_ok());
}

#[test]
fn renderer_derives_context_endpoint_and_rejects_unordered_or_inconsistent_facts() {
    let user = render(input("user", vec![])).unwrap();
    let endpoint: Value = serde_json::from_slice(user.endpoint_json()).unwrap();
    assert_eq!(
        endpoint["socketAddress"],
        json!({"kind":"linux_user_runtime_relative","value":"graft/user/worker.sock"})
    );

    let unordered = input(
        "system",
        vec![workload("beta", 'b'), workload("alpha", 'a')],
    );
    assert!(matches!(
        render(unordered),
        Err(ManifestError::WorkloadOrder)
    ));

    let mut inconsistent = workload("alpha", 'a');
    inconsistent["containerName"] = "other".into();
    assert!(matches!(
        render(input("system", vec![inconsistent])),
        Err(ManifestError::WorkloadIdentityMismatch)
    ));
}

#[test]
fn renderer_input_refuses_unknown_secret_bearing_fields() {
    let mut value = serde_json::to_value(input("system", vec![])).unwrap();
    value["token"] = "secret".into();
    assert!(serde_json::from_value::<RenderInput>(value).is_err());
}
