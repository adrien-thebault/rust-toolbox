use std::time::Duration;

use http::StatusCode;
use toolbox_web::{ClientIpTrustPolicy, rate_limit::RateLimitConfig};

use super::{app, app_with, state};
use crate::{call, get as get_req, post_json};

/// The credential routes are throttled and the others are not.
///
/// This is the regression test for a real defect: the module documented a login
/// rate limit that `auth_router` never attached, so an attacker got as many
/// password guesses per second as argon2 allowed.
#[tokio::test]
async fn the_credential_routes_are_throttled_and_the_others_are_not() {
    let app = app_with(
        state(),
        &RateLimitConfig::new(1, Duration::from_secs(60), ClientIpTrustPolicy::hops(1)),
    );
    let wrong = r#"{"username":"ada","password":"nope"}"#;

    let (first, _) = call(app.clone(), post_json("/auth/login", wrong)).await;
    assert_eq!(
        first.status(),
        StatusCode::UNAUTHORIZED,
        "the first attempt is a normal rejection"
    );

    let (throttled, body) = call(app.clone(), post_json("/auth/login", wrong)).await;
    assert_eq!(throttled.status(), StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert!(
        throttled.headers().contains_key("retry-after"),
        "the wait the limiter computed has to reach the client, or it can only guess"
    );
    assert!(body.contains("RATE_LIMITED"), "{body}");

    // `/auth/me` is not a credential check and is not throttled.
    let (me, _) = call(app, get_req("/auth/me")).await;
    assert_eq!(
        me.status(),
        StatusCode::UNAUTHORIZED,
        "reached, not throttled"
    );
}

#[tokio::test]
async fn a_correct_login_returns_a_session_with_a_refresh_token() {
    let body = r#"{"username":"ada","password":"hunter2"}"#;
    let (res, text) = call(app(state()), post_json("/auth/login", body)).await;

    assert_eq!(res.status(), StatusCode::OK, "{text}");
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["token_type"], "Bearer");
    assert_eq!(v["expires_in"], 900, "fifteen minutes, not twelve hours");
    assert!(v["access_token"].as_str().is_some_and(|t| t.contains('.')));
    assert!(
        v["refresh_token"].as_str().is_some_and(|t| t.contains('.')),
        "a refresh token is always issued now"
    );
}

#[tokio::test]
async fn a_wrong_password_is_a_401_problem() {
    let body = r#"{"username":"ada","password":"wrong"}"#;
    let (res, text) = call(app(state()), post_json("/auth/login", body)).await;

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(res.headers()["content-type"], toolbox_core::PROBLEM_JSON);
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["code"], "UNAUTHENTICATED");
}

#[tokio::test]
async fn an_unknown_user_fails_exactly_like_a_wrong_password() {
    let unknown = r#"{"username":"nobody","password":"hunter2"}"#;
    let wrong = r#"{"username":"ada","password":"wrong"}"#;
    let (a, _) = call(app(state()), post_json("/auth/login", unknown)).await;
    let (b, _) = call(app(state()), post_json("/auth/login", wrong)).await;
    assert_eq!(a.status(), b.status());
}

#[tokio::test]
async fn a_refresh_token_can_be_redeemed_for_a_new_session() {
    let app = app(state());
    let login = r#"{"username":"ada","password":"hunter2"}"#;
    let (_, text) = call(app.clone(), post_json("/auth/login", login)).await;
    let session: serde_json::Value = serde_json::from_str(&text).unwrap();
    let refresh = session["refresh_token"].as_str().unwrap().to_owned();

    let body = format!(r#"{{"refresh_token":"{refresh}"}}"#);
    let (res, text) = call(app.clone(), post_json("/auth/refresh", &body)).await;
    assert_eq!(res.status(), StatusCode::OK, "{text}");

    let rotated: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(
        rotated["access_token"]
            .as_str()
            .is_some_and(|t| t.contains('.'))
    );
    assert!(rotated["refresh_token"].as_str().is_some());
}

/// Stateless refresh has no server-side record, so a changed credential
/// fingerprint is the revocation mechanism.
#[tokio::test]
async fn a_changed_credential_fingerprint_invalidates_a_refresh_token() {
    let state = state();
    *state.epoch.lock().unwrap() = Some("epoch-1".to_owned());
    let app = app(state.clone());

    let login = r#"{"username":"ada","password":"hunter2"}"#;
    let (_, text) = call(app.clone(), post_json("/auth/login", login)).await;
    let refresh = serde_json::from_str::<serde_json::Value>(&text).unwrap()["refresh_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let body = format!(r#"{{"refresh_token":"{refresh}"}}"#);

    // Same fingerprint: still good.
    let (ok, _) = call(app.clone(), post_json("/auth/refresh", &body)).await;
    assert_eq!(ok.status(), StatusCode::OK);

    // The password changed.
    *state.epoch.lock().unwrap() = Some("epoch-2".to_owned());
    let (rejected, _) = call(app, post_json("/auth/refresh", &body)).await;
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logout_is_a_no_op_the_client_drives() {
    let (res, _) = call(app(state()), post_json("/auth/logout", "")).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn me_is_401_when_anonymous() {
    let (res, _) = call(app(state()), get_req("/auth/me")).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}
