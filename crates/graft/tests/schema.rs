use std::collections::BTreeSet;

use graft::config::schema::ContainerConfig;
use serde_json::Value;

const TRACKED_SCHEMA: &str = include_str!("../schema/graft-v1.schema.json");

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
        ("root", &["config", "deploy", "name", "version"][..]),
        (
            "Config",
            &["container", "filesystem", "network", "runtime", "service"][..],
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
        ("Deploy", &["enable", "target"][..]),
        ("Filesystem", &["volumes"][..]),
        ("FilesystemVolume", &["mode", "source", "target"][..]),
        ("Network", &["publish"][..]),
        ("Runtime", &["command", "mode", "packages"][..]),
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
    ];

    for (definition, expected) in expected_properties {
        let value = if definition == "root" {
            &schema
        } else {
            &schema["$defs"][definition]
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

    let lifecycle_values = schema["$defs"]["ServiceLifecycle"]["oneOf"]
        .as_array()
        .expect("ServiceLifecycle should define variants")
        .iter()
        .map(|variant| variant["const"].clone())
        .collect::<Vec<_>>();
    assert_eq!(
        lifecycle_values,
        vec![
            serde_json::json!("long-running"),
            serde_json::json!("job"),
            serde_json::json!("setup"),
        ]
    );
}
