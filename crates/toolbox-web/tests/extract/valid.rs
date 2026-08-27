use axum::{Router, routing::post};
use garde::Validate;
use http::StatusCode;
use serde::Deserialize;
use toolbox_web::extract::ValidJson;

use crate::{call, post_json};

#[derive(Debug, Deserialize, Validate)]
struct Address {
    #[garde(length(min = 3))]
    postcode: String,
}

#[derive(Debug, Deserialize, Validate)]
struct NewUser {
    #[garde(length(min = 2, max = 50))]
    name: String,
    #[garde(email)]
    email: String,
    #[garde(range(min = 18))]
    age: u8,
    #[garde(dive)]
    address: Address,
}

fn app() -> Router {
    Router::new().route(
        "/users",
        post(|ValidJson(u): ValidJson<NewUser>| async move { u.name }),
    )
}

#[tokio::test]
async fn a_valid_body_reaches_the_handler() {
    let body = r#"{"name":"Ada","email":"ada@example.test","age":36,
                   "address":{"postcode":"12345"}}"#;
    let (res, text) = call(app(), post_json("/users", body)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(text, "Ada");
}

/// The whole reason validation is in the toolbox: the failure has to name the
/// fields, in a shape a frontend can use to highlight inputs.
#[tokio::test]
async fn an_invalid_body_names_every_bad_field() {
    let body = r#"{"name":"A","email":"nope","age":12,
                   "address":{"postcode":"1"}}"#;
    let (res, text) = call(app(), post_json("/users", body)).await;

    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(res.headers()["content-type"], toolbox_core::PROBLEM_JSON);

    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["code"], "VALIDATION_FAILED");
    let metadata = v["metadata"].as_object().unwrap();
    assert!(metadata.contains_key("name"), "{text}");
    assert!(metadata.contains_key("email"), "{text}");
    assert!(metadata.contains_key("age"), "{text}");
}

/// A nested field's path has to survive, or a frontend cannot tell which input
/// to mark. This is the property that decided the crate - see ADR 0003.
#[tokio::test]
async fn a_nested_field_keeps_its_path() {
    let body = r#"{"name":"Ada","email":"ada@example.test","age":36,
                   "address":{"postcode":"1"}}"#;
    let (_, text) = call(app(), post_json("/users", body)).await;
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    let metadata = v["metadata"].as_object().unwrap();
    assert!(
        metadata.keys().any(|k| k.contains("postcode")),
        "the nested path must be addressable: {text}"
    );
}

#[tokio::test]
async fn a_body_that_is_not_json_is_a_400_problem_not_a_500() {
    let (res, text) = call(app(), post_json("/users", "{not json")).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["code"], "MALFORMED_BODY");
}

#[tokio::test]
async fn a_body_missing_a_field_is_a_400_problem() {
    let (res, text) = call(app(), post_json("/users", r#"{"name":"Ada"}"#)).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    assert_eq!(res.headers()["content-type"], toolbox_core::PROBLEM_JSON);
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["code"], "MALFORMED_BODY");
}
