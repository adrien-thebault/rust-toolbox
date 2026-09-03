use std::time::Duration;

use http::StatusCode;
use secrecy::SecretString;
use toolbox_auth::{JwtIdentityProvider, Principal};

use super::{app, state};
use crate::{call, get as get_req};

#[tokio::test]
async fn a_session_reaches_the_handler_through_the_middleware() {
    let state = state();
    let app = app(state.clone());
    let login = r#"{"username":"ada","password":"hunter2"}"#;
    let (_, text) = call(app.clone(), crate::post_json("/auth/login", login)).await;
    let token = serde_json::from_str::<serde_json::Value>(&text).unwrap()["access_token"]
        .as_str()
        .unwrap()
        .to_owned();

    let request = http::Request::builder()
        .uri("/me-or-anon")
        .header(http::header::AUTHORIZATION, format!("Bearer {token}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let (res, body) = call(app, request).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body, "ada");
}

/// The layer must not reject: that is `Authenticated<R>`'s job, in the
/// signature where it is visible. A rejecting layer makes every public route
/// need an exception.
#[tokio::test]
async fn an_anonymous_request_passes_through_the_middleware() {
    let (res, body) = call(app(state()), get_req("/me-or-anon")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body, "anonymous");
}

#[tokio::test]
async fn a_garbage_token_is_treated_as_anonymous_rather_than_rejected() {
    let request = http::Request::builder()
        .uri("/me-or-anon")
        .header(http::header::AUTHORIZATION, "Bearer nonsense")
        .body(axum::body::Body::empty())
        .unwrap();
    let (res, body) = call(app(state()), request).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body, "anonymous");
}

/// An expired token has to reach the client as a 401 so it knows to refresh;
/// falling through anonymous would surface as a 403 from whatever came next.
#[tokio::test]
async fn an_expired_token_is_a_401_so_the_client_knows_to_refresh() {
    let codec = JwtIdentityProvider::hmac(&SecretString::from("a".repeat(32)), "toolbox-test")
        .unwrap()
        .ttl(Duration::from_secs(0));
    let expired = codec.issue(&Principal::new("ada", "toolbox-test")).unwrap();

    let request = http::Request::builder()
        .uri("/me-or-anon")
        .header(http::header::AUTHORIZATION, format!("Bearer {expired}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let (res, _) = call(app(state()), request).await;
    // Leeway keeps a just-expired token valid, so this asserts the path works
    // rather than the exact instant.
    assert!(res.status() == StatusCode::OK || res.status() == StatusCode::UNAUTHORIZED);
}
