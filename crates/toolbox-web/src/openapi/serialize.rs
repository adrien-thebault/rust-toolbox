//! Serializing a spec with a stable key order.

use std::collections::BTreeMap;

use serde_json::Value;
use utoipa::openapi::OpenApi;

/// Serialize a spec with **stable key ordering**.
///
/// This is what makes the drift guard work at all: `git diff --exit-code` is
/// useless if key order shuffles between runs, and it will, because some of
/// the maps underneath are hash-ordered.
///
/// # Arguments
///
/// * `api` - The spec to serialize. Key order is made stable here, which is
///   what lets CI diff the committed file.
///
/// # Errors
/// [`serde_json::Error`] when the document cannot be serialized, which would
/// mean utoipa produced something invalid.
pub fn serialize_openapi(api: &OpenApi) -> Result<String, serde_json::Error> {
    let value = serde_json::to_value(api)?;
    serde_json::to_string_pretty(&canonicalize(value))
}

/// Rebuild every object with its keys sorted.
///
/// Done explicitly rather than relying on `serde_json::Map` being a `BTreeMap`,
/// because that depends on whether anything in the workspace enabled
/// `serde_json`'s `preserve_order` feature - and feature unification means that
/// is not this crate's decision to make.
///
/// # Arguments
///
/// * `value` - The document to rebuild with sorted keys. Done explicitly rather
///   than relying on `serde_json::Map` being a `BTreeMap`, because that depends
///   on a feature any crate in the graph can turn off.
fn canonicalize(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut sorted: BTreeMap<String, Value> = BTreeMap::new();
            for (k, v) in map {
                sorted.insert(k, canonicalize(v));
            }
            Value::Object(sorted.into_iter().collect())
        }
        Value::Array(items) => Value::Array(items.into_iter().map(canonicalize).collect()),
        other => other,
    }
}
