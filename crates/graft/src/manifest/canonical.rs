//! Canonical JSON and SHA-256 support for Graft discovery documents.

use serde::Serialize;
use serde_json::Value;
use sha2::{Digest as _, Sha256};

use super::ManifestError;

/// Serializes a value as Graft canonical JSON version 1.
///
/// Object keys are lexicographically ordered by `serde_json`'s map
/// representation. Floating-point values are forbidden by the manifest schema.
///
/// # Errors
///
/// Returns an error when serialization fails or the value contains a float.
pub(crate) fn to_canonical_json<T: Serialize>(value: &T) -> Result<Vec<u8>, ManifestError> {
    let value = serde_json::to_value(value).map_err(ManifestError::Serialize)?;
    reject_floats(&value)?;
    serde_json::to_vec(&value).map_err(ManifestError::Serialize)
}

/// Returns the lowercase SHA-256 digest of canonical JSON bytes.
pub(crate) fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut encoded = String::with_capacity(64);
    for byte in digest {
        use std::fmt::Write as _;
        write!(encoded, "{byte:02x}").expect("writing to String cannot fail");
    }
    encoded
}

fn reject_floats(value: &Value) -> Result<(), ManifestError> {
    match value {
        Value::Number(number) if number.is_f64() => Err(ManifestError::FloatingPoint),
        Value::Array(values) => values.iter().try_for_each(reject_floats),
        Value::Object(values) => values.values().try_for_each(reject_floats),
        Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => Ok(()),
    }
}
