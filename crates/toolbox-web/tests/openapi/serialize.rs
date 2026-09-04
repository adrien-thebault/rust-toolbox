use toolbox_web::openapi::{serialize_openapi, with_standard_errors};
use utoipa::OpenApi;

use super::Api;

/// The drift guard is `git diff --exit-code` against a committed file, which
/// is useless if key order shuffles between runs.
#[test]
fn the_serialization_is_byte_identical_across_runs() {
    let mut a = Api::openapi();
    with_standard_errors(&mut a);
    let mut b = Api::openapi();
    with_standard_errors(&mut b);

    assert_eq!(
        serialize_openapi(&a).unwrap(),
        serialize_openapi(&b).unwrap()
    );
}

#[test]
fn the_serialization_has_its_keys_sorted() {
    let spec = serialize_openapi(&Api::openapi()).unwrap();
    let value: serde_json::Value = serde_json::from_str(&spec).unwrap();
    assert_sorted(&value);
}

fn assert_sorted(value: &serde_json::Value) {
    match value {
        serde_json::Value::Object(map) => {
            let keys: Vec<&String> = map.keys().collect();
            let mut expected = keys.clone();
            expected.sort();
            assert_eq!(keys, expected, "object keys are not sorted");
            for v in map.values() {
                assert_sorted(v);
            }
        }
        serde_json::Value::Array(items) => items.iter().for_each(assert_sorted),
        _ => {}
    }
}
