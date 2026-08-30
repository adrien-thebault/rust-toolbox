use std::time::{Duration, SystemTime, UNIX_EPOCH};

use toolbox_schedule::{Clock, ManualClock, SystemClock};

#[test]
fn the_system_clock_reports_the_real_time() {
    let before = SystemTime::now();
    let now = SystemClock.now();
    assert!(now >= before);
}

#[test]
fn a_manual_clock_starts_at_the_epoch_and_only_moves_when_moved() {
    let clock = ManualClock::new();
    assert_eq!(clock.now(), UNIX_EPOCH);
    clock.advance(Duration::from_secs(90));
    assert_eq!(clock.now(), UNIX_EPOCH + Duration::from_secs(90));
    assert_eq!(clock.millis(), 90_000);
}

#[test]
fn a_manual_clock_can_start_anywhere() {
    let t = UNIX_EPOCH + Duration::from_secs(1_700_000_000);
    assert_eq!(ManualClock::at(t).now(), t);
}

/// The reason the clock is a port: a scheduler test asserts a year of cron
/// fires without taking a year, or a second.
#[tokio::test]
async fn a_sleep_on_a_manual_clock_waits_for_the_clock_not_the_wall() {
    let clock = ManualClock::new();
    let sleeper = {
        let clock = clock.clone();
        tokio::spawn(async move {
            clock.sleep(Duration::from_secs(3600)).await;
            clock.millis()
        })
    };

    tokio::task::yield_now().await;
    assert!(
        !sleeper.is_finished(),
        "still asleep: the clock has not moved"
    );

    clock.advance(Duration::from_secs(3600));
    let woke_at = tokio::time::timeout(Duration::from_secs(5), sleeper)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(woke_at, 3_600_000);
}

#[tokio::test]
async fn a_partial_advance_does_not_wake_a_sleeper() {
    let clock = ManualClock::new();
    let sleeper = {
        let clock = clock.clone();
        tokio::spawn(async move { clock.sleep(Duration::from_secs(60)).await })
    };

    tokio::task::yield_now().await;
    clock.advance(Duration::from_secs(30));
    tokio::task::yield_now().await;
    assert!(!sleeper.is_finished(), "half way is not there yet");

    clock.advance(Duration::from_secs(30));
    tokio::time::timeout(Duration::from_secs(5), sleeper)
        .await
        .unwrap()
        .unwrap();
}
