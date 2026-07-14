use std::collections::BTreeSet;

use graft::config::schema::ContainerConfig;
use serde_json::Value;

const TRACKED_SCHEMA: &str = include_str!("../schema/graft-v1.schema.json");

fn assert_enum_values(schema: &Value, definition: &str, expected: &[&str]) {
    let actual = schema["$defs"][definition]["oneOf"]
        .as_array()
        .unwrap_or_else(|| panic!("{definition} should define variants"))
        .iter()
        .map(|variant| {
            variant["const"]
                .as_str()
                .unwrap_or_else(|| panic!("{definition} variants should define constants"))
        })
        .collect::<BTreeSet<_>>();
    let expected = expected.iter().copied().collect::<BTreeSet<_>>();

    assert_eq!(actual, expected, "unexpected variants in {definition}");
}

fn assert_dependency_target_variants(schema: &Value) {
    let actual = schema["$defs"]["DependencyTarget"]["anyOf"]
        .as_array()
        .expect("DependencyTarget should define variants")
        .iter()
        .map(|variant| {
            variant["$ref"]
                .as_str()
                .expect("DependencyTarget variants should define references")
        })
        .collect::<BTreeSet<_>>();
    let expected = [
        "#/$defs/ExternalUnitDependencyTarget",
        "#/$defs/WorkloadDependencyTarget",
    ]
    .into_iter()
    .collect::<BTreeSet<_>>();

    assert_eq!(actual, expected);
}

#[test]
fn generated_schema_matches_tracked_file() {
    let schema = schemars::schema_for!(ContainerConfig);
    let mut generated = serde_json::to_string_pretty(&schema)
        .expect("generated Graft schema should serialize as JSON");
    generated.push('\n');

    assert_eq!(
        generated, TRACKED_SCHEMA,
        "regenerate with `cargo run --example generate-schema > schema/graft-v1.schema.json`"
    );
}

#[test]
fn schema_exposes_only_supported_fields() {
    let schema: Value =
        serde_json::from_str(TRACKED_SCHEMA).expect("tracked Graft schema should be valid JSON");

    let expected_properties = [
        (
            "root",
            &["config", "dependencies", "deploy", "name", "version"][..],
        ),
        (
            "Config",
            &[
                "container",
                "filesystem",
                "network",
                "runtime",
                "security",
                "service",
            ][..],
        ),
        (
            "Container",
            &[
                "environment",
                "environmentFile",
                "group",
                "hostname",
                "user",
                "workingDir",
            ][..],
        ),
        (
            "Dependency",
            &["lifecycle", "ordering", "requirement", "target"][..],
        ),
        ("Deploy", &["activation", "enable", "target"][..]),
        ("Device", &["source"][..]),
        ("ExternalUnitDependencyTarget", &["externalUnit"][..]),
        (
            "Filesystem",
            &["binds", "devices", "readOnly", "tmpfs", "volumes"][..],
        ),
        ("FilesystemBind", &["readOnly", "source", "target"][..]),
        ("FilesystemTmpfs", &["mode", "size", "target"][..]),
        ("FilesystemVolume", &["name", "readOnly", "target"][..]),
        ("Network", &["container", "mode", "publish"][..]),
        ("Runtime", &["command", "mode", "packages"][..]),
        (
            "Security",
            &["addCapabilities", "dropCapabilities", "noNewPrivileges"][..],
        ),
        (
            "Service",
            &[
                "lifecycle",
                "restart",
                "restartSec",
                "timeoutStartSec",
                "timeoutStopSec",
            ][..],
        ),
        ("WorkloadDependencyTarget", &["workload"][..]),
    ];

    for (definition, expected) in expected_properties {
        let value = match definition {
            "root" => &schema,
            "Deploy" => &schema["properties"]["deploy"],
            _ => &schema["$defs"][definition],
        };
        let properties = value["properties"]
            .as_object()
            .unwrap_or_else(|| panic!("{definition} should define object properties"));
        let actual = properties
            .keys()
            .map(String::as_str)
            .collect::<BTreeSet<_>>();
        let expected = expected.iter().copied().collect::<BTreeSet<_>>();

        assert_eq!(actual, expected, "unexpected fields in {definition} schema");
        assert_eq!(value["additionalProperties"], false);
    }

    assert_dependency_target_variants(&schema);
    assert_enum_values(&schema, "DependencyRequirement", &["optional", "required"]);
    assert_enum_values(&schema, "DependencyOrdering", &["after", "before"]);
    assert_enum_values(&schema, "DependencyLifecycle", &["bound", "part-of"]);
    assert_enum_values(&schema, "DeployActivation", &["startup"]);
    assert_enum_values(
        &schema,
        "ServiceLifecycle",
        &["job", "long-running", "setup"],
    );
    assert_enum_values(&schema, "NetworkMode", &["container", "none"]);
}

#[test]
fn schema_constrains_typed_filesystem_and_device_fields() {
    let schema: Value =
        serde_json::from_str(TRACKED_SCHEMA).expect("tracked Graft schema should be valid JSON");

    assert_eq!(
        schema["$defs"]["Device"]["properties"]["source"]["pattern"],
        "^[A-Za-z][A-Za-z0-9._-]*[A-Za-z0-9]/[A-Za-z][A-Za-z0-9._-]*[A-Za-z0-9]=[A-Za-z0-9](?:[A-Za-z0-9._-]*[A-Za-z0-9])?$"
    );
    assert_eq!(schema["$defs"]["Device"]["required"][0], "source");
    assert_eq!(
        schema["$defs"]["Filesystem"]["properties"]["tmpfs"]["items"]["$ref"],
        "#/$defs/FilesystemTmpfs"
    );
    assert_eq!(
        schema["$defs"]["FilesystemTmpfs"]["properties"]["mode"]["pattern"],
        r"^[01]?[0-7]{3}$"
    );
    assert_eq!(
        schema["$defs"]["FilesystemTmpfs"]["properties"]["size"]["pattern"],
        r"^[1-9][0-9]*[KMGT]?$"
    );
    assert_eq!(
        schema["$defs"]["FilesystemVolume"]["properties"]["name"]["maxLength"],
        128
    );
}

#[test]
fn schema_exposes_secure_defaults_and_typed_relaxations() {
    let schema: Value =
        serde_json::from_str(TRACKED_SCHEMA).expect("tracked Graft schema should be valid JSON");

    assert_eq!(
        schema["$defs"]["Security"]["properties"]["dropCapabilities"]["minItems"],
        1
    );
    assert_eq!(
        schema["$defs"]["Security"]["properties"]["dropCapabilities"]["maxItems"],
        1
    );
    assert_eq!(
        schema["$defs"]["Security"]["properties"]["dropCapabilities"]["items"]["pattern"],
        "^all$"
    );
    assert_eq!(
        schema["$defs"]["Security"]["properties"]["addCapabilities"]["items"]["pattern"],
        "^CAP_[A-Z][A-Z0-9_]*$"
    );
    assert_eq!(
        schema["$defs"]["Security"]["properties"]["addCapabilities"]["uniqueItems"],
        true
    );
    assert!(schema["$defs"]["Security"]["properties"]["noNewPrivileges"]
        .get("const")
        .is_none());
    assert!(schema["$defs"]["Filesystem"]["properties"]["readOnly"]
        .get("const")
        .is_none());
    assert!(schema["required"]
        .as_array()
        .is_some_and(|required| required.iter().any(|field| field == "deploy")));
    assert!(schema["properties"]["deploy"]["required"]
        .as_array()
        .is_some_and(|required| required.iter().any(|field| field == "target")));
}
