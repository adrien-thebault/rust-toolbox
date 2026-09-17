use axum::{Router, body::Body, http::Request, routing::post};
use http::StatusCode;
use toolbox_server::stack::StackConfig;
use toolbox_web::apply_http_stack;
use tower::ServiceExt as _;

async fn echo(body: String) -> String {
    body
}

#[tokio::test]
async fn http_stack_applies_stack_configs_body_limit() {
    let app = apply_http_stack(
        Router::new().route("/", post(echo)),
        StackConfig::default().max_body_bytes(Some(4)),
    );
    let response = app
        .oneshot(
            Request::post("/")
                .body(Body::from("12345"))
                .expect("valid request"),
        )
        .await
        .expect("router is infallible");

    assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
}

#[tokio::test]
async fn http_stack_can_disable_the_body_limit() {
    let app = apply_http_stack(
        Router::new().route("/", post(echo)),
        StackConfig::default().max_body_bytes(None),
    );
    let response = app
        .oneshot(
            Request::post("/")
                .body(Body::from("12345"))
                .expect("valid request"),
        )
        .await
        .expect("router is infallible");

    assert_eq!(response.status(), StatusCode::OK);
}
