use std::time::Duration;

use cloudevents::AttributesReader as _;
use toolbox_cluster::{
    EventBus, KeyValueStore, LockManager, Scope, Topic, deployment::Adapter, signal,
};
use toolbox_cluster_postgres::{MIGRATIONS, OutboxBus, PostgresKeyValue, PostgresLocks};

use crate::require_postgres;

/// Every adapter here exists so the guard accepts it under clustering. If one
/// declared anything else it would be pointless.
#[tokio::test]
async fn every_adapter_declares_itself_shared() {
    let db = require_postgres!();
    assert_eq!(PostgresKeyValue::new(db.clone()).scope(), Scope::Shared);
    assert_eq!(PostgresLocks::new(db.clone()).scope(), Scope::Shared);
    assert_eq!(OutboxBus::new(db).scope(), Scope::Shared);
}

#[tokio::test]
async fn the_key_value_store_round_trips() {
    let db = require_postgres!();
    db.migrate(MIGRATIONS).await.unwrap();
    let kv = PostgresKeyValue::new(db);

    let key = format!("test:{}", uuid::Uuid::now_v7());
    kv.set(&key, b"value".to_vec(), None).await.unwrap();
    assert_eq!(kv.get(&key).await.unwrap(), Some(b"value".to_vec()));

    kv.set(&key, b"replaced".to_vec(), None).await.unwrap();
    assert_eq!(
        kv.get(&key).await.unwrap(),
        Some(b"replaced".to_vec()),
        "upsert, not a duplicate"
    );

    kv.delete(&key).await.unwrap();
    assert_eq!(kv.get(&key).await.unwrap(), None);
}

/// The property refresh-token rotation is built on, now against a shared
/// store where two *replicas* could race rather than two tasks.
#[tokio::test]
async fn take_returns_the_value_exactly_once_across_concurrent_callers() {
    let db = require_postgres!();
    db.migrate(MIGRATIONS).await.unwrap();
    let kv = std::sync::Arc::new(PostgresKeyValue::new(db));

    let key = format!("test:{}", uuid::Uuid::now_v7());
    kv.set(&key, b"single-use".to_vec(), None).await.unwrap();

    let mut handles = Vec::new();
    for _ in 0..8 {
        let kv = std::sync::Arc::clone(&kv);
        let key = key.clone();
        handles.push(tokio::spawn(async move { kv.take(&key).await.unwrap() }));
    }

    let mut winners = 0;
    for handle in handles {
        if handle.await.unwrap().is_some() {
            winners += 1;
        }
    }
    assert_eq!(winners, 1, "DELETE .. RETURNING is atomic");
}

#[tokio::test]
async fn an_expired_key_is_not_returned_even_before_it_is_purged() {
    let db = require_postgres!();
    db.migrate(MIGRATIONS).await.unwrap();
    let kv = PostgresKeyValue::new(db);

    let key = format!("test:{}", uuid::Uuid::now_v7());
    kv.set(&key, b"gone".to_vec(), Some(Duration::from_millis(1)))
        .await
        .unwrap();
    tokio::time::sleep(Duration::from_millis(50)).await;

    assert_eq!(
        kv.get(&key).await.unwrap(),
        None,
        "expiry is enforced on read"
    );
    assert_eq!(kv.take(&key).await.unwrap(), None);
    kv.purge_expired().await.unwrap();
}

#[tokio::test]
async fn a_lock_is_held_against_a_second_taker() {
    let db = require_postgres!();
    db.migrate(MIGRATIONS).await.unwrap();
    let locks = PostgresLocks::new(db);
    let key = format!("test:{}", uuid::Uuid::now_v7());

    let held = locks.try_lock(&key, Duration::from_secs(30)).await.unwrap();
    assert!(held.is_some());

    let second = locks.try_lock(&key, Duration::from_secs(30)).await.unwrap();
    assert!(second.is_none(), "contention is Ok(None), not an error");
}

#[tokio::test]
async fn different_lock_keys_do_not_contend() {
    let db = require_postgres!();
    db.migrate(MIGRATIONS).await.unwrap();
    let locks = PostgresLocks::new(db);
    let a = locks
        .try_lock(
            &format!("a:{}", uuid::Uuid::now_v7()),
            Duration::from_secs(5),
        )
        .await;
    let b = locks
        .try_lock(
            &format!("b:{}", uuid::Uuid::now_v7()),
            Duration::from_secs(5),
        )
        .await;
    assert!(a.unwrap().is_some() && b.unwrap().is_some());
}

/// The point of the outbox: the event and the change commit together or not
/// at all.
#[tokio::test]
async fn an_event_enqueued_in_a_rolled_back_transaction_is_not_published() {
    use diesel::prelude::*;

    let _guard = crate::OUTBOX.lock().await;
    let db = require_postgres!();
    db.migrate(MIGRATIONS).await.unwrap();
    let bus = OutboxBus::new(db.clone());
    bus.drain_batch().await.unwrap(); // clear anything left over

    let topic = Topic::new("test.rollback");
    let result: Result<(), toolbox_db::DbError> = db
        .run(move |c: &mut diesel::pg::PgConnection| {
            c.transaction(|c| {
                OutboxBus::enqueue(c, &topic, &signal("rolled.back", "/test").unwrap())?;
                // Whatever came next failed.
                Err::<(), diesel::result::Error>(diesel::result::Error::RollbackTransaction)
            })
            .map_err(toolbox_db::DbError::from)
        })
        .await;
    assert!(result.is_err());

    let drained = bus.drain_batch().await.unwrap();
    assert!(
        !drained.iter().any(|(_, e)| e.ty() == "rolled.back"),
        "the event rolled back with the change"
    );
}

#[tokio::test]
async fn a_published_event_is_drained_exactly_once() {
    let _guard = crate::OUTBOX.lock().await;
    let db = require_postgres!();
    db.migrate(MIGRATIONS).await.unwrap();
    let bus = OutboxBus::new(db);
    bus.drain_batch().await.unwrap();

    let topic = Topic::new("test.once");
    let event = signal("happened", "/test").unwrap();
    bus.publish(&topic, event).await.unwrap();

    let first = bus.drain_batch().await.unwrap();
    assert_eq!(
        first.iter().filter(|(_, e)| e.ty() == "happened").count(),
        1
    );

    let second = bus.drain_batch().await.unwrap();
    assert_eq!(
        second.iter().filter(|(_, e)| e.ty() == "happened").count(),
        0,
        "a drained event is marked published"
    );
}

/// Saying at-least-once out loud is what stops somebody assuming otherwise.
#[tokio::test]
async fn the_outbox_declares_at_least_once_delivery() {
    let db = require_postgres!();
    let caps = OutboxBus::new(db).capabilities();
    assert_eq!(caps.delivery, toolbox_cluster::Delivery::AtLeastOnce);
    assert!(caps.durable);
    assert!(caps.replay.is_some());
}

/// A backlog that only grows means the relay is not running, which is the
/// thing to alert on.
#[tokio::test]
async fn the_backlog_is_observable() {
    let _guard = crate::OUTBOX.lock().await;
    let db = require_postgres!();
    db.migrate(MIGRATIONS).await.unwrap();
    let bus = OutboxBus::new(db);
    bus.drain_batch().await.unwrap();

    let before = bus.backlog().await.unwrap();
    bus.publish(&Topic::new("test.backlog"), signal("x", "/t").unwrap())
        .await
        .unwrap();
    assert_eq!(bus.backlog().await.unwrap(), before + 1);

    bus.drain_batch().await.unwrap();
    assert_eq!(bus.backlog().await.unwrap(), 0);
}

/// Subscribing would race with the relay, so it is refused rather than
/// silently handing out a second consumer.
#[tokio::test]
async fn subscribing_directly_is_refused_because_the_relay_owns_the_queue() {
    let db = require_postgres!();
    let bus = OutboxBus::new(db);
    let result = bus
        .subscribe(&Topic::new("t"), toolbox_cluster::StartPosition::Now)
        .await;
    assert!(result.is_err());
}

/// A lease that expires is takeable by somebody else, which is what makes a
/// hung holder recoverable rather than permanent - and is exactly what
/// `pg_advisory_lock` could not have given us.
#[tokio::test]
async fn an_expired_lease_is_taken_over() {
    let db = require_postgres!();
    db.migrate(MIGRATIONS).await.unwrap();
    let locks = PostgresLocks::new(db);
    let key = format!("test:{}", uuid::Uuid::now_v7());

    let held = locks
        .try_lock(&key, Duration::from_millis(1))
        .await
        .unwrap()
        .unwrap();
    std::mem::forget(held); // the holder died without releasing

    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(
        locks
            .try_lock(&key, Duration::from_secs(30))
            .await
            .unwrap()
            .is_some(),
        "the lease expired, so the work can proceed elsewhere"
    );
}

#[tokio::test]
async fn dropping_the_guard_releases_the_lock() {
    let db = require_postgres!();
    db.migrate(MIGRATIONS).await.unwrap();
    let locks = PostgresLocks::new(db);
    let key = format!("test:{}", uuid::Uuid::now_v7());

    let held = locks
        .try_lock(&key, Duration::from_secs(30))
        .await
        .unwrap()
        .unwrap();
    drop(held);

    // The release is spawned, since Drop cannot await.
    for _ in 0..50 {
        if locks
            .try_lock(&key, Duration::from_secs(30))
            .await
            .unwrap()
            .is_some()
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("the lock was never released");
}

/// A long lease is also a long outage when the holder dies, so a job that runs
/// longer than its lease renews rather than asking for more up front.
#[tokio::test]
async fn a_lease_is_renewable_only_by_whoever_holds_it() {
    let db = require_postgres!();
    db.migrate(MIGRATIONS).await.unwrap();
    let locks = PostgresLocks::new(db);
    let key = format!("test:{}", uuid::Uuid::now_v7());

    let _held = locks
        .try_lock(&key, Duration::from_secs(60))
        .await
        .unwrap()
        .unwrap();
    assert!(
        !locks
            .renew(&key, "somebody-else", Duration::from_secs(60))
            .await
            .unwrap()
    );
}

/// The bug a real server caught: with `pg_advisory_lock` the second caller was
/// handed the same pooled session, where the lock is re-entrant, and won.
#[tokio::test]
async fn a_second_taker_loses_even_when_it_gets_the_same_pooled_connection() {
    let db = require_postgres!();
    db.migrate(MIGRATIONS).await.unwrap();
    // A pool of one guarantees both callers share a connection.
    let locks = PostgresLocks::new(db);
    let key = format!("test:{}", uuid::Uuid::now_v7());

    let _first = locks
        .try_lock(&key, Duration::from_secs(60))
        .await
        .unwrap()
        .unwrap();
    for _ in 0..5 {
        assert!(
            locks
                .try_lock(&key, Duration::from_secs(60))
                .await
                .unwrap()
                .is_none(),
            "the lease belongs to its owner, not to a connection"
        );
    }
}

#[tokio::test]
async fn expired_leases_can_be_purged() {
    let db = require_postgres!();
    db.migrate(MIGRATIONS).await.unwrap();
    let locks = PostgresLocks::new(db);

    let held = locks
        .try_lock(
            &format!("test:{}", uuid::Uuid::now_v7()),
            Duration::from_millis(1),
        )
        .await
        .unwrap()
        .unwrap();
    std::mem::forget(held);

    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(locks.purge_expired().await.unwrap() >= 1);
}
