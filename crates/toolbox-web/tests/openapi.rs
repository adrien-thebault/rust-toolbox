use toolbox_core::PROBLEM_JSON;
use toolbox_web::openapi::{bearer_security, dump_openapi, with_standard_errors};
use utoipa::OpenApi;

#[derive(utoipa::ToSchema, serde::Serialize)]
struct Todo {
    id: i32,
    title: String,
}

#[utoipa::path(get, path = "/api/todos/{id}", responses((status = 200, body = Todo)))]
#[allow(dead_code)]
fn get_todo() {}

#[utoipa::path(post, path = "/api/todos", responses((status = 201, body = Todo)))]
#[allow(dead_code)]
fn create_todo() {}

#[derive(OpenApi)]
#[openapi(paths(get_todo, create_todo), components(schemas(Todo)))]
struct Api;

/// Without this, each handler hand-annotates seven responses, which in
/// practice means most annotate none and the spec claims endpoints cannot fail.
#[test]
fn every_operation_gains_the_standard_error_responses() {
    let mut api = Api::openapi();
    with_standard_errors(&mut api);

    for (path, item) in &api.paths.paths {
        for operation in [item.get.as_ref(), item.post.as_ref()]
            .into_iter()
            .flatten()
        {
            for status in ["400", "401", "403", "404", "409", "429", "500"] {
                assert!(
                    operation.responses.responses.contains_key(status),
                    "{path} is missing a {status} response"
                );
            }
        }
    }
}

#[test]
fn the_error_responses_are_problem_json() {
    let mut api = Api::openapi();
    with_standard_errors(&mut api);
    let spec = dump_openapi(&api).unwrap();
    assert!(
        spec.contains(PROBLEM_JSON),
        "the standard errors declare their media type"
    );
}

#[test]
fn a_hand_annotated_response_is_not_overwritten() {
    let mut api = Api::openapi();
    with_standard_errors(&mut api);
    let item = api.paths.paths.get("/api/todos/{id}").unwrap();
    assert!(
        item.get
            .as_ref()
            .unwrap()
            .responses
            .responses
            .contains_key("200")
    );
}

#[test]
fn bearer_security_is_declared_once_on_the_document() {
    let mut api = Api::openapi();
    bearer_security(&mut api);
    let spec = dump_openapi(&api).unwrap();
    assert!(spec.contains("\"bearer\""), "{spec}");
    assert!(spec.contains("\"JWT\""), "{spec}");
}

/// The drift guard is `git diff --exit-code` against a committed file, which
/// is useless if key order shuffles between runs.
#[test]
fn the_dump_is_byte_identical_across_runs() {
    let mut a = Api::openapi();
    with_standard_errors(&mut a);
    let mut b = Api::openapi();
    with_standard_errors(&mut b);

    assert_eq!(dump_openapi(&a).unwrap(), dump_openapi(&b).unwrap());
}

#[test]
fn the_dump_has_its_keys_sorted() {
    let spec = dump_openapi(&Api::openapi()).unwrap();
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
