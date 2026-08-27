use std::time::Duration;

use toolbox_server::shutdown::{Shutdown, ShutdownConfig};

#[test]
fn a_fresh_handle_is_ready_and_not_shutting_down() {
    let s = Shutdown::new();
    assert!(s.readiness().is_ready());
    assert!(!s.is_shutting_down());
}

#[test]
fn begin_fails_readiness_before_anything_stops_serving() {
    let s = Shutdown::new();
    let readiness = s.readiness();
    s.begin();
    assert!(!readiness.is_ready());
    assert!(s.is_shutting_down());
}

#[tokio::test]
async fn watchers_are_told_that_shutdown_started() {
    let s = Shutdown::new();
    let mut rx = s.watch();
    assert!(!*rx.borrow());
    s.begin();
    rx.changed().await.unwrap();
    assert!(*rx.borrow());
}

/// Step 2 is the one everyone omits: readiness must fail, and then the process
/// must keep serving for `drain_delay` so the load balancer notices.
#[tokio::test(start_paused = true)]
async fn drain_fails_readiness_first_then_waits() {
    let s = Shutdown::new();
    let readiness = s.readiness();
    let cfg = ShutdownConfig {
        drain_delay: Duration::from_secs(5),
        ..ShutdownConfig::default()
    };

    let started = tokio::time::Instant::now();
    let handle = tokio::spawn({
        let s = s.clone();
        async move { s.drain(cfg).await }
    });

    tokio::task::yield_now().await;
    assert!(
        !readiness.is_ready(),
        "readiness fails before the delay, not after"
    );

    handle.await.unwrap();
    assert!(started.elapsed() >= Duration::from_secs(5));
}

#[test]
fn the_defaults_leave_time_for_a_load_balancer_to_notice() {
    let cfg = ShutdownConfig::default();
    assert_eq!(cfg.drain_delay, Duration::from_secs(5));
    assert_eq!(cfg.drain_timeout, Duration::from_secs(30));
}
