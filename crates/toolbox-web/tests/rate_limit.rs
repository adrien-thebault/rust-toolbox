use http::StatusCode;
use toolbox_cluster::deployment::{Adapter, Scope};
use toolbox_web::{
    TrustedHops,
    rate_limit::{ForwardedForKeyExtractor, RateLimitAdapter, error_response_handler},
};
use tower_governor::{GovernorError, key_extractor::KeyExtractor};

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

/// The limiter must key on the same client IP the logs do, so counting from
/// the right of `X-Forwarded-For` is what stops a client choosing its bucket.
#[test]
fn the_extractor_keys_on_the_trusted_hop_not_the_client_supplied_entry() {
    let extractor = ForwardedForKeyExtractor::new(TrustedHops(1));
    let key = extractor
        .extract(&request("1.1.1.1, 2.2.2.2", [10, 0, 0, 1]))
        .unwrap();
    assert_eq!(key.to_string(), "2.2.2.2");
}

#[test]
fn the_extractor_falls_back_to_the_peer_when_the_header_is_short() {
    let extractor = ForwardedForKeyExtractor::new(TrustedHops(2));
    let key = extractor
        .extract(&request("1.1.1.1", [10, 0, 0, 1]))
        .unwrap();
    assert_eq!(key.to_string(), "10.0.0.1");
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
    assert_eq!(res.headers()["content-type"], toolbox_core::PROBLEM_JSON);
}

#[test]
fn an_unidentifiable_client_is_a_400_not_a_429() {
    let res = error_response_handler(GovernorError::UnableToExtractKey);
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

/// Per-process limiting degrades under replicas rather than breaking, so the
/// adapter declares `LocalDegraded` and the guard warns rather than refusing.
#[test]
fn the_adapter_declares_itself_degraded_under_clustering() {
    assert!(matches!(
        RateLimitAdapter.scope(),
        Scope::LocalDegraded { .. }
    ));
    assert_eq!(RateLimitAdapter.name(), "tower_governor");
}
