use std::{sync::Arc, time::Duration};

use toolbox_cluster::{InMemoryKvStore, KvStore, KvStoreError};
use toolbox_web::{
    extract::IdempotencyKey,
    idempotency::{Idempotency, IdempotencyOutcome, StoredResponse, in_flight_error},
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
    Idempotency::new(Arc::new(InMemoryKvStore::default()))
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
        IdempotencyOutcome::Fresh(_)
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
        IdempotencyOutcome::InFlight
    ));

    let err = in_flight_error();
    assert_eq!(err.status(), http::StatusCode::CONFLICT);
    assert_eq!(err.problem().code.as_deref(), Some("IDEMPOTENCY_IN_FLIGHT"));
}

#[tokio::test]
async fn a_retry_after_completion_replays_the_recorded_response() {
    let idem = store();
    let IdempotencyOutcome::Fresh(claim) = idem.claim(&key("abc"), "/pay").await.unwrap() else {
        panic!("expected a fresh claim");
    };
    idem.record(claim, &response()).await.unwrap();

    match idem.claim(&key("abc"), "/pay").await.unwrap() {
        IdempotencyOutcome::Replay(stored) => {
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
    let IdempotencyOutcome::Fresh(claim) = idem.claim(&key("abc"), "/pay").await.unwrap() else {
        panic!("expected a fresh claim");
    };
    idem.record(claim, &response()).await.unwrap();

    assert!(matches!(
        idem.claim(&key("abc"), "/refund").await.unwrap(),
        IdempotencyOutcome::Fresh(_)
    ));
}

/// A 5xx is not an outcome worth replaying, and leaving the key claimed makes
/// the retry - the entire reason a key was sent - impossible.
#[tokio::test]
async fn releasing_a_failed_request_lets_the_caller_retry() {
    let idem = store();
    let IdempotencyOutcome::Fresh(claim) = idem.claim(&key("abc"), "/pay").await.unwrap() else {
        panic!("expected a fresh claim");
    };
    idem.release(claim).await.unwrap();

    assert!(matches!(
        idem.claim(&key("abc"), "/pay").await.unwrap(),
        IdempotencyOutcome::Fresh(_)
    ));
}

/// Corrupt state must not permit a potentially duplicate side effect.
#[tokio::test]
async fn an_undecodable_record_fails_closed() {
    let kv = Arc::new(InMemoryKvStore::default());
    let idem = Idempotency::new(kv.clone());
    kv.set("toolbox:idem:/pay:abc", b"this is not json".to_vec(), None)
        .await
        .unwrap();

    let err = idem.claim(&key("abc"), "/pay").await.unwrap_err();
    assert_eq!(err.status(), http::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        err.problem().code.as_deref(),
        Some("IDEMPOTENCY_RECORD_INVALID")
    );
}

/// A store failure and a conflict lead a client to opposite behaviours, so the
/// store failure has to be a distinct, retryable 503.
#[tokio::test]
async fn a_store_failure_is_a_distinct_retryable_error() {
    let idem = Idempotency::new(Arc::new(AlwaysFails));
    let err = idem.claim(&key("abc"), "/pay").await.unwrap_err();

    assert_eq!(err.status(), http::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        err.problem().code.as_deref(),
        Some("IDEMPOTENCY_STORE_UNAVAILABLE")
    );
}

/// A store that repeatedly loses an entry between `add` and `get` must not
/// make `claim` spin and hammer it forever.
#[tokio::test]
async fn a_perpetually_disappearing_entry_fails_after_one_retry() {
    let idem = Idempotency::new(Arc::new(AlwaysMissing));
    let err = idem.claim(&key("abc"), "/pay").await.unwrap_err();

    assert_eq!(err.status(), http::StatusCode::SERVICE_UNAVAILABLE);
    assert_eq!(
        err.problem().code.as_deref(),
        Some("IDEMPOTENCY_STORE_UNAVAILABLE")
    );
}

/// A key-value store whose every operation reports a backend failure.
struct AlwaysFails;

/// A store that reports a competing write but never exposes its value.
struct AlwaysMissing;

#[async_trait::async_trait]
impl KvStore for AlwaysMissing {
    async fn get(&self, _: &str) -> Result<Option<Vec<u8>>, KvStoreError> {
        Ok(None)
    }
    async fn set(&self, _: &str, _: Vec<u8>, _: Option<Duration>) -> Result<(), KvStoreError> {
        Ok(())
    }
    async fn add(&self, _: &str, _: Vec<u8>, _: Option<Duration>) -> Result<bool, KvStoreError> {
        Ok(false)
    }
    async fn replace_if_matches(
        &self,
        _: &str,
        _: &[u8],
        _: Option<Vec<u8>>,
        _: Option<Duration>,
    ) -> Result<bool, KvStoreError> {
        Ok(false)
    }
    async fn take(&self, _: &str) -> Result<Option<Vec<u8>>, KvStoreError> {
        Ok(None)
    }
    async fn delete(&self, _: &str) -> Result<(), KvStoreError> {
        Ok(())
    }
}

#[async_trait::async_trait]
impl KvStore for AlwaysFails {
    async fn get(&self, _: &str) -> Result<Option<Vec<u8>>, KvStoreError> {
        Err(KvStoreError::Backend("down".to_owned()))
    }
    async fn set(&self, _: &str, _: Vec<u8>, _: Option<Duration>) -> Result<(), KvStoreError> {
        Err(KvStoreError::Backend("down".to_owned()))
    }
    async fn add(&self, _: &str, _: Vec<u8>, _: Option<Duration>) -> Result<bool, KvStoreError> {
        Err(KvStoreError::Backend("down".to_owned()))
    }
    async fn replace_if_matches(
        &self,
        _: &str,
        _: &[u8],
        _: Option<Vec<u8>>,
        _: Option<Duration>,
    ) -> Result<bool, KvStoreError> {
        Err(KvStoreError::Backend("down".to_owned()))
    }
    async fn take(&self, _: &str) -> Result<Option<Vec<u8>>, KvStoreError> {
        Err(KvStoreError::Backend("down".to_owned()))
    }
    async fn delete(&self, _: &str) -> Result<(), KvStoreError> {
        Err(KvStoreError::Backend("down".to_owned()))
    }
}

#[tokio::test]
async fn different_keys_do_not_interfere() {
    let idem = store();
    idem.claim(&key("a"), "/pay").await.unwrap();
    assert!(matches!(
        idem.claim(&key("b"), "/pay").await.unwrap(),
        IdempotencyOutcome::Fresh(_)
    ));
}

#[tokio::test]
async fn an_expired_owner_cannot_overwrite_or_release_a_new_claim() {
    let idem = store().claim_ttl(Duration::from_millis(10));
    let IdempotencyOutcome::Fresh(stale) = idem.claim(&key("abc"), "/pay").await.unwrap() else {
        panic!("expected a fresh claim");
    };

    tokio::time::sleep(Duration::from_millis(60)).await;
    let IdempotencyOutcome::Fresh(current) = idem.claim(&key("abc"), "/pay").await.unwrap() else {
        panic!("expected the expired claim to be replaced");
    };

    let err = idem.record(stale, &response()).await.unwrap_err();
    assert_eq!(
        err.problem().code.as_deref(),
        Some("IDEMPOTENCY_CLAIM_LOST")
    );

    idem.release(current).await.unwrap();
    assert!(matches!(
        idem.claim(&key("abc"), "/pay").await.unwrap(),
        IdempotencyOutcome::Fresh(_)
    ));
}
