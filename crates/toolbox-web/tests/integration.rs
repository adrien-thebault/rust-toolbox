//! One harness per crate; the module tree mirrors `src/`.
#![allow(
    missing_docs,
    clippy::missing_panics_doc,
    clippy::missing_errors_doc,
    clippy::unnecessary_wraps
)]

mod auth;
mod captcha;
mod client_ip;
mod error;
mod extract;
mod health;
mod idempotency;
mod links;
mod openapi;
mod realtime;

use axum::{Router, body::Body};
use http::{Request, Response};
use tower::ServiceExt as _;

/// Drive a router in process and read the whole response.
pub async fn call(app: Router, req: Request<Body>) -> (Response<Body>, String) {
    let res = app.oneshot(req).await.unwrap();
    let (parts, body) = res.into_parts();
    let bytes = axum::body::to_bytes(body, usize::MAX).await.unwrap();
    let text = String::from_utf8(bytes.to_vec()).unwrap();
    (Response::from_parts(parts, Body::empty()), text)
}

/// A GET request.
pub fn get(uri: &str) -> Request<Body> {
    Request::builder().uri(uri).body(Body::empty()).unwrap()
}

/// A POST request with a JSON body.
pub fn post_json(uri: &str, body: &str) -> Request<Body> {
    Request::builder()
        .method("POST")
        .uri(uri)
        .header("content-type", "application/json")
        .body(Body::from(body.to_owned()))
        .unwrap()
}
