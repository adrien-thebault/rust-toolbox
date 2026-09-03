use std::time::Duration;

use http::StatusCode;
use toolbox_web::{TrustedHops, auth::LoginLimit};

use super::{app_with, state};
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
        LoginLimit {
            burst: 1,
            replenish_every: Duration::from_secs(60),
            hops: TrustedHops::default(),
        },
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
