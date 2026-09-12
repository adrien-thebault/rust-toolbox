use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use toolbox_server::server::{Health, Probe, poll_check};

#[test]
fn is_healthy_is_true_for_healthy_alone() {
    assert!(Health::Healthy.is_healthy());
    assert!(!Health::Starting.is_healthy());
    assert!(!Health::Degraded.is_healthy());
    assert!(!Health::ShuttingDown.is_healthy());
}

#[test]
fn a_polled_check_reports_the_name_it_was_given() {
    let probe = poll_check("thing", Duration::from_secs(5), (), |()| async { true });
    assert_eq!(probe.check.name(), "thing");
}

#[tokio::test(start_paused = true)]
async fn a_polled_check_starts_unhealthy_then_tracks_the_probe() {
    let flag = Arc::new(AtomicBool::new(true));
    let Probe { check, poll } = poll_check(
        "thing",
        Duration::from_secs(5),
        Arc::clone(&flag),
        |flag| async move { flag.load(Ordering::Relaxed) },
    );
    assert!(!check.is_healthy(), "no tick has run yet");

    tokio::spawn(poll);
    tokio::task::yield_now().await;
    assert!(check.is_healthy(), "the first tick fires immediately");

    flag.store(false, Ordering::Relaxed);
    tokio::time::advance(Duration::from_secs(5)).await;
    tokio::task::yield_now().await;
    assert!(
        !check.is_healthy(),
        "the second tick, one interval later, picked up the flip"
    );
}
