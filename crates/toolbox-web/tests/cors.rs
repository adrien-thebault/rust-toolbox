use axum::{Router, routing::get};
use http::{Request, StatusCode, header};
use toolbox_web::cors::{cors, cors_localhost};

use crate::call;

fn app(layer: tower_http::cors::CorsLayer) -> Router {
    Router::new()
        .route("/x", get(|| async { "ok" }))
        .layer(layer)
}

fn with_origin(origin: &str) -> Request<axum::body::Body> {
    Request::builder()
        .uri("/x")
        .header(header::ORIGIN, origin)
        .body(axum::body::Body::empty())
        .unwrap()
}

#[tokio::test]
async fn a_listed_origin_is_reflected_with_credentials() {
    let app = app(cors(&["https://app.example".to_owned()]));
    let (res, _) = call(app, with_origin("https://app.example")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(
        res.headers()["access-control-allow-origin"],
        "https://app.example"
    );
    assert_eq!(res.headers()["access-control-allow-credentials"], "true");
}

#[tokio::test]
async fn an_unlisted_origin_gets_no_allow_header() {
    let app = app(cors(&["https://app.example".to_owned()]));
    let (res, _) = call(app, with_origin("https://evil.example")).await;
    assert!(!res.headers().contains_key("access-control-allow-origin"));
}

#[tokio::test]
async fn cors_localhost_reflects_any_loopback_port() {
    let app = app(cors_localhost(&["https://app.example".to_owned()]));
    for origin in [
        "http://localhost:5173",
        "http://localhost:4200",
        "http://127.0.0.1:3000",
        "https://app.example",
    ] {
        let (res, _) = call(app.clone(), with_origin(origin)).await;
        assert_eq!(
            res.headers()["access-control-allow-origin"],
            origin,
            "{origin} should be allowed"
        );
    }
}

#[tokio::test]
async fn cors_localhost_still_refuses_a_public_origin() {
    let app = app(cors_localhost(&[]));
    let (res, _) = call(app, with_origin("https://evil.example")).await;
    assert!(!res.headers().contains_key("access-control-allow-origin"));
}
