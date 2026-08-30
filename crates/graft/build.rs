use std::{env, fs, path::Path};

use serde::Deserialize;

#[derive(Deserialize)]
struct WorkerApiRange {
    major: u16,
    min_minor: u16,
    max_minor: u16,
}

fn main() {
    println!("cargo:rerun-if-changed=worker-api-range.json");

    let source = fs::read_to_string("worker-api-range.json")
        .expect("worker-api-range.json must exist and be readable");
    let range: WorkerApiRange = serde_json::from_str(&source)
        .expect("worker-api-range.json must contain a valid worker API range");
    assert!(
        range.min_minor <= range.max_minor,
        "worker-api-range.json must have min_minor <= max_minor"
    );

    let output = format!(
        "pub const PROTOCOL_MAJOR: u16 = {};\n\
pub const PROTOCOL_MIN_MINOR: u16 = {};\n\
pub const PROTOCOL_MAX_MINOR: u16 = {};\n",
        range.major, range.min_minor, range.max_minor
    );
    let output_path = Path::new(&env::var_os("OUT_DIR").expect("OUT_DIR must be set"))
        .join("worker_api_range.rs");
    fs::write(output_path, output).expect("failed to write generated worker API constants");
}
