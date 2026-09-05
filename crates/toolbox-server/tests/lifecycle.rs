mod health;
mod shutdown;
mod startup;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use toolbox_server::lifecycle::{Health, HealthCheck, LifecycleHandle, Shutdown};

/// A check flipped by the test, shared with the handle via an `Arc` so both
/// sides see the same flag.
struct Toggle(Arc<AtomicBool>);

impl HealthCheck for Toggle {
    fn name(&self) -> &'static str {
        "toggle"
    }
    fn is_healthy(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

fn toggle(healthy: bool) -> (Arc<AtomicBool>, Box<dyn HealthCheck>) {
    let flag = Arc::new(AtomicBool::new(healthy));
    (Arc::clone(&flag), Box::new(Toggle(flag)))
}

#[test]
fn with_no_checks_a_fresh_handle_is_healthy_at_once() {
    let handle = LifecycleHandle::new(Shutdown::new());
    assert_eq!(handle.current(), Health::Healthy);
}

#[test]
fn a_failing_check_starts_the_process_as_starting_not_degraded() {
    let (_flag, check) = toggle(false);
    let handle = LifecycleHandle::new(Shutdown::new()).with_checks(vec![check]);
    assert_eq!(handle.current(), Health::Starting);
}

#[test]
fn a_check_that_fails_after_passing_once_reads_as_degraded() {
    let (flag, check) = toggle(true);
    let handle = LifecycleHandle::new(Shutdown::new()).with_checks(vec![check]);

    assert_eq!(
        handle.current(),
        Health::Healthy,
        "passes once, so it latches"
    );
    flag.store(false, Ordering::SeqCst);
    assert_eq!(
        handle.current(),
        Health::Degraded,
        "was healthy before, so a later failure is a regression, not a cold start"
    );
    flag.store(true, Ordering::SeqCst);
    assert_eq!(handle.current(), Health::Healthy);
}

#[test]
fn shutdown_wins_over_every_check() {
    let shutdown = Shutdown::new();
    let handle = LifecycleHandle::new(shutdown.clone());
    assert_eq!(handle.current(), Health::Healthy);

    shutdown.begin();
    assert_eq!(handle.current(), Health::ShuttingDown);
}
