use std::sync::Arc;

use toolbox_cluster::InMemoryKeyValue;
use toolbox_web::{
    extract::IdempotencyKey,
    idempotency::{Claim, Idempotency, StoredResponse, in_flight_error},
};

fn key(s: &str) -> IdempotencyKey {
    // The extractor is the only constructor, so a test builds one through it.
    use axum::extract::FromRequestParts as _;
    let request = http::Request::builder()
        .uri("/x")
        .header("idempotency-key", s)
        .body(())
        .unwrap();
    let (mut parts, ()) = request.into_parts();
    let extracted = futures_executor::block_on(
        toolbox_web::extract::Idempotent::from_request_parts(&mut parts, &()),
    )
    .unwrap();
    extracted.0.expect("a key")
}

fn store() -> Idempotency {
    Idempotency::new(Arc::new(InMemoryKeyValue::default()))
}

fn response() -> StoredResponse {
    StoredResponse {
        status: 201,
        body: br#"{"id":7}"#.to_vec(),
        content_type: "application/json".to_owned(),
    }
}

#[tokio::test]
async fn a_first_request_claims_the_key() {
    let idem = store();
    assert!(matches!(
        idem.claim(&key("abc"), "/pay").await.unwrap(),
        Claim::Fresh
    ));
}

/// The correct answer to "did my first request succeed?" is not "here, have
/// another one".
#[tokio::test]
async fn a_second_request_while_the_first_runs_is_a_conflict() {
    let idem = store();
    idem.claim(&key("abc"), "/pay").await.unwrap();
    assert!(matches!(
        idem.claim(&key("abc"), "/pay").await.unwrap(),
        Claim::InFlight
    ));

    let err = in_flight_error();
    assert_eq!(err.status(), http::StatusCode::CONFLICT);
    assert_eq!(err.problem().code.as_deref(), Some("IDEMPOTENCY_IN_FLIGHT"));
}

#[tokio::test]
async fn a_retry_after_completion_replays_the_recorded_response() {
    let idem = store();
    idem.claim(&key("abc"), "/pay").await.unwrap();
    idem.record(&key("abc"), "/pay", &response()).await.unwrap();

    match idem.claim(&key("abc"), "/pay").await.unwrap() {
        Claim::Replay(stored) => {
            assert_eq!(stored.status, 201);
            assert_eq!(stored.body, br#"{"id":7}"#);
            assert_eq!(stored.content_type, "application/json");
        }
        other => panic!("expected a replay, got {other:?}"),
    }
}

/// Two endpoints given the same client-chosen key are two different
/// operations, and replaying one's response for the other would be worse than
/// not replaying at all.
#[tokio::test]
async fn the_same_key_on_a_different_route_is_a_different_operation() {
    let idem = store();
    idem.claim(&key("abc"), "/pay").await.unwrap();
    idem.record(&key("abc"), "/pay", &response()).await.unwrap();

    assert!(matches!(
        idem.claim(&key("abc"), "/refund").await.unwrap(),
        Claim::Fresh
    ));
}

/// A 5xx is not an outcome worth replaying, and leaving the key claimed makes
/// the retry - the entire reason a key was sent - impossible.
#[tokio::test]
async fn releasing_a_failed_request_lets_the_caller_retry() {
    let idem = store();
    idem.claim(&key("abc"), "/pay").await.unwrap();
    idem.release(&key("abc"), "/pay").await.unwrap();

    assert!(matches!(
        idem.claim(&key("abc"), "/pay").await.unwrap(),
        Claim::Fresh
    ));
}

#[tokio::test]
async fn different_keys_do_not_interfere() {
    let idem = store();
    idem.claim(&key("a"), "/pay").await.unwrap();
    assert!(matches!(
        idem.claim(&key("b"), "/pay").await.unwrap(),
        Claim::Fresh
    ));
}
