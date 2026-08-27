use std::time::Duration;

use toolbox_cluster::{InProcessLocks, LockManager};

#[tokio::test]
async fn a_lock_can_be_taken_and_is_then_held() {
    let locks = InProcessLocks::new();
    let guard = locks
        .try_lock("job", Duration::from_secs(60))
        .await
        .unwrap();
    assert!(guard.is_some());

    let second = locks
        .try_lock("job", Duration::from_secs(60))
        .await
        .unwrap();
    assert!(second.is_none(), "contention is Ok(None), not an error");
}

#[tokio::test]
async fn dropping_the_guard_releases_the_lock() {
    let locks = InProcessLocks::new();
    let guard = locks
        .try_lock("job", Duration::from_secs(60))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(guard.key(), "job");
    drop(guard);

    assert!(
        locks
            .try_lock("job", Duration::from_secs(60))
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn different_keys_do_not_contend() {
    let locks = InProcessLocks::new();
    let _a = locks
        .try_lock("a", Duration::from_secs(60))
        .await
        .unwrap()
        .unwrap();
    assert!(
        locks
            .try_lock("b", Duration::from_secs(60))
            .await
            .unwrap()
            .is_some()
    );
}

/// A holder that dies without releasing must not block the work forever: that
/// is how a scheduled job silently never runs again.
#[tokio::test]
async fn an_expired_lease_can_be_taken_by_someone_else() {
    let locks = InProcessLocks::new();
    let guard = locks
        .try_lock("job", Duration::from_millis(20))
        .await
        .unwrap()
        .unwrap();
    std::mem::forget(guard); // the holder died without releasing

    tokio::time::sleep(Duration::from_millis(60)).await;
    assert!(
        locks
            .try_lock("job", Duration::from_secs(60))
            .await
            .unwrap()
            .is_some()
    );
}

/// Releasing a lease that has already been taken by someone else would steal
/// their lock, so the guard checks it still owns the key.
#[tokio::test]
async fn a_stale_guard_does_not_release_someone_elses_lock() {
    let locks = InProcessLocks::new();
    let stale = locks
        .try_lock("job", Duration::from_millis(20))
        .await
        .unwrap()
        .unwrap();
    tokio::time::sleep(Duration::from_millis(60)).await;

    let fresh = locks
        .try_lock("job", Duration::from_secs(60))
        .await
        .unwrap()
        .unwrap();
    drop(stale);

    assert!(
        locks
            .try_lock("job", Duration::from_secs(60))
            .await
            .unwrap()
            .is_none()
    );
    drop(fresh);
}

#[tokio::test]
async fn the_adapter_declares_what_it_actually_does() {
    let caps = InProcessLocks::new().capabilities();
    assert!(!caps.shared, "two replicas would each take the same lock");
    assert!(caps.leased);
}
