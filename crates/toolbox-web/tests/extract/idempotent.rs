use axum::{Router, body::Body, routing::post};
use http::{Request, StatusCode};
use toolbox_web::extract::{IDEMPOTENCY_KEY, Idempotent, idempotency_key_max_len};

use crate::call;

fn app() -> Router {
    Router::new().route(
        "/pay",
        post(|Idempotent(key): Idempotent| async move {
            key.map_or_else(|| "none".to_owned(), |k| k.to_string())
        }),
    )
}

fn post_with(key: Option<&str>) -> Request<Body> {
    let mut b = Request::builder().method("POST").uri("/pay");
    if let Some(k) = key {
        b = b.header(IDEMPOTENCY_KEY, k);
    }
    b.body(Body::empty()).unwrap()
}

/// the IETF draft's header name, not a bespoke one, so a client
/// library that already knows the convention works unchanged.
#[test]
fn the_header_is_the_ietf_draft_name() {
    assert_eq!(IDEMPOTENCY_KEY.as_str(), "idempotency-key");
}

#[tokio::test]
async fn a_key_reaches_the_handler() {
    let (res, body) = call(app(), post_with(Some("abc-123"))).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body, "abc-123");
}

#[tokio::test]
async fn no_key_is_not_an_error() {
    let (res, body) = call(app(), post_with(None)).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body, "none");
}

#[tokio::test]
async fn an_empty_key_is_refused() {
    let (res, body) = call(app(), post_with(Some(""))).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["code"], "INVALID_IDEMPOTENCY_KEY");
}

/// Without a cap the header is free storage on someone else's server.
#[tokio::test]
async fn an_over_long_key_is_refused() {
    let long = "x".repeat(idempotency_key_max_len() + 1);
    let (res, _) = call(app(), post_with(Some(&long))).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_key_at_the_limit_is_accepted() {
    let at_limit = "x".repeat(idempotency_key_max_len());
    let (res, _) = call(app(), post_with(Some(&at_limit))).await;
    assert_eq!(res.status(), StatusCode::OK);
}
