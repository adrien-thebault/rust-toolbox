use std::time::Duration;

use axum::{Router, routing::get};
use http::StatusCode;
use toolbox_web::{
    ClientIpTrustPolicy, PRIVATE_RANGES,
    rate_limit::{ForwardedForKeyExtractor, RateLimitConfig, error_response_handler},
};
use tower_governor::{GovernorError, key_extractor::KeyExtractor};

use crate::call;

fn request(xff: &str, peer: [u8; 4]) -> http::Request<()> {
    let mut req = http::Request::builder().uri("/x");
    if !xff.is_empty() {
        req = req.header("x-forwarded-for", xff);
    }
    let mut req = req.body(()).unwrap();
    req.extensions_mut()
        .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
            peer, 40_000,
        ))));
    req
}

/// The limiter must key on the same client IP the logs do. Walking from the
/// trusted proxy towards the client prevents a forged leading entry winning.
#[test]
fn the_extractor_stops_at_the_first_untrusted_address() {
    let extractor =
        ForwardedForKeyExtractor::new(ClientIpTrustPolicy::BehindProxies(PRIVATE_RANGES.to_vec()));
    let key = extractor
        .extract(&request("1.1.1.1, 2.2.2.2, 10.0.0.2", [10, 0, 0, 1]))
        .unwrap();
    assert_eq!(key.to_string(), "2.2.2.2");
}

#[test]
fn the_extractor_ignores_headers_from_an_untrusted_peer() {
    let extractor =
        ForwardedForKeyExtractor::new(ClientIpTrustPolicy::BehindProxies(PRIVATE_RANGES.to_vec()));
    let key = extractor
        .extract(&request("1.1.1.1", [203, 0, 113, 1]))
        .unwrap();
    assert_eq!(key.to_string(), "203.0.113.1");
}

/// A throttled request is not the one response in the API that does not look
/// like every other error.
#[test]
fn a_rejection_becomes_a_problem_document_with_retry_after() {
    let res = error_response_handler(GovernorError::TooManyRequests {
        wait_time: 7,
        headers: None,
    });
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(res.headers()["retry-after"], "7");
    assert_eq!(res.headers()["ratelimit-remaining"], "0");
    assert_eq!(res.headers()["content-type"], toolbox_error::PROBLEM_JSON);
}

#[test]
fn an_unidentifiable_client_is_a_400_not_a_429() {
    let res = error_response_handler(GovernorError::UnableToExtractKey);
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

/// The layer `auth_router` and any other throttled route are built from: after
/// the burst, the next request is a 429 rather than reaching the handler.
#[tokio::test]
async fn the_layer_throttles_once_the_burst_is_spent() {
    let app = Router::new()
        .route("/x", get(|| async { "ok" }))
        .layer(RateLimitConfig::new(1, Duration::from_mins(1), ClientIpTrustPolicy::Peer).layer());

    let req = || {
        let mut r = http::Request::builder()
            .uri("/x")
            .body(axum::body::Body::empty())
            .unwrap();
        r.extensions_mut()
            .insert(axum::extract::ConnectInfo(std::net::SocketAddr::from((
                [127, 0, 0, 1],
                5000,
            ))));
        r
    };

    let (first, _) = call(app.clone(), req()).await;
    assert_eq!(first.status(), StatusCode::OK);

    let (second, body) = call(app, req()).await;
    assert_eq!(second.status(), StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert!(body.contains("RATE_LIMITED"), "{body}");
}
