use std::time::Duration;

use toolbox_cluster::{InMemoryKeyValue, KeyValueStore};

#[tokio::test]
async fn a_value_round_trips() {
    let kv = InMemoryKeyValue::default();
    kv.set("k", b"v".to_vec(), None).await.unwrap();
    assert_eq!(kv.get("k").await.unwrap(), Some(b"v".to_vec()));
}

#[tokio::test]
async fn a_missing_key_is_none_rather_than_an_error() {
    assert_eq!(InMemoryKeyValue::default().get("nope").await.unwrap(), None);
}

#[tokio::test]
async fn delete_removes_the_key_and_is_idempotent() {
    let kv = InMemoryKeyValue::default();
    kv.set("k", b"v".to_vec(), None).await.unwrap();
    kv.delete("k").await.unwrap();
    kv.delete("k").await.unwrap();
    assert_eq!(kv.get("k").await.unwrap(), None);
}

/// The capability refresh-token rotation is built on: a second `take` must see
/// nothing, or a replayed token is silently accepted.
#[tokio::test]
async fn take_returns_the_value_exactly_once() {
    let kv = InMemoryKeyValue::default();
    kv.set("token", b"secret".to_vec(), None).await.unwrap();

    assert_eq!(kv.take("token").await.unwrap(), Some(b"secret".to_vec()));
    assert_eq!(
        kv.take("token").await.unwrap(),
        None,
        "a replay finds nothing"
    );
    assert_eq!(kv.get("token").await.unwrap(), None);
}

#[tokio::test]
async fn concurrent_takes_produce_exactly_one_winner() {
    let kv = std::sync::Arc::new(InMemoryKeyValue::default());
    kv.set("token", b"secret".to_vec(), None).await.unwrap();

    let mut handles = Vec::new();
    for _ in 0..16 {
        let kv = std::sync::Arc::clone(&kv);
        handles.push(tokio::spawn(async move { kv.take("token").await.unwrap() }));
    }

    let mut winners = 0;
    for h in handles {
        if h.await.unwrap().is_some() {
            winners += 1;
        }
    }
    assert_eq!(winners, 1, "exactly one caller consumed the token");
}

#[tokio::test]
async fn taking_a_missing_key_is_none() {
    assert_eq!(
        InMemoryKeyValue::default().take("nope").await.unwrap(),
        None
    );
}

#[tokio::test]
async fn an_expired_entry_is_gone() {
    let kv = InMemoryKeyValue::default();
    kv.set("k", b"v".to_vec(), Some(Duration::from_millis(30)))
        .await
        .unwrap();
    assert!(kv.get("k").await.unwrap().is_some());

    tokio::time::sleep(Duration::from_millis(80)).await;
    assert_eq!(kv.get("k").await.unwrap(), None, "the ttl was honoured");
}

#[tokio::test]
async fn the_adapter_declares_what_it_actually_does() {
    let caps = InMemoryKeyValue::default().capabilities();
    assert!(
        caps.atomic_take,
        "asserted by concurrent_takes_produce_exactly_one_winner"
    );
    assert!(
        caps.atomic_add,
        "asserted by concurrent_adds_produce_exactly_one_winner"
    );
    assert!(caps.ttl);
    assert!(!caps.durable);
    assert!(!caps.shared, "entries are invisible to other replicas");
}

#[tokio::test]
async fn add_creates_a_key_once_and_reports_which_call_won() {
    let kv = InMemoryKeyValue::default();
    assert!(kv.add("k", b"first".to_vec(), None).await.unwrap());
    assert!(
        !kv.add("k", b"second".to_vec(), None).await.unwrap(),
        "a live key is not overwritten"
    );
    assert_eq!(kv.get("k").await.unwrap(), Some(b"first".to_vec()));
}

#[tokio::test]
async fn add_overwrites_an_expired_key() {
    let kv = InMemoryKeyValue::default();
    kv.add("k", b"stale".to_vec(), Some(Duration::from_millis(10)))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(60)).await;

    assert!(
        kv.add("k", b"fresh".to_vec(), None).await.unwrap(),
        "an expired entry counts as absent"
    );
    assert_eq!(kv.get("k").await.unwrap(), Some(b"fresh".to_vec()));
}

#[tokio::test]
async fn concurrent_adds_produce_exactly_one_winner() {
    let kv = std::sync::Arc::new(InMemoryKeyValue::default());

    let mut handles = Vec::new();
    for _ in 0..16 {
        let kv = std::sync::Arc::clone(&kv);
        handles.push(tokio::spawn(async move {
            kv.add("claim", b"x".to_vec(), None).await.unwrap()
        }));
    }

    let mut winners = 0;
    for h in handles {
        if h.await.unwrap() {
            winners += 1;
        }
    }
    assert_eq!(winners, 1, "exactly one caller created the key");
}

/// moka's `remove` hands back an entry that has expired but not yet been
/// evicted, and a `take` that returns an expired single-use token is a token
/// that never expires. Found by a realtime ticket test.
#[tokio::test]
async fn take_does_not_return_an_expired_entry() {
    let kv = InMemoryKeyValue::default();
    kv.set("k", b"v".to_vec(), Some(Duration::from_millis(10)))
        .await
        .unwrap();

    tokio::time::sleep(Duration::from_millis(60)).await;
    assert_eq!(
        kv.take("k").await.unwrap(),
        None,
        "expiry is enforced on take, not only on get"
    );
}
