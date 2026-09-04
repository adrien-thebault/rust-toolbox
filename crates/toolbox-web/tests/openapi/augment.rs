use toolbox_core::PROBLEM_JSON;
use toolbox_web::openapi::{bearer_security, serialize_openapi, with_standard_errors};
use utoipa::OpenApi;

use super::Api;

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
    let spec = serialize_openapi(&api).unwrap();
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
    let spec = serialize_openapi(&api).unwrap();
    assert!(spec.contains("\"bearer\""), "{spec}");
    assert!(spec.contains("\"JWT\""), "{spec}");
}
