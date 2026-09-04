mod shutdown;
mod startup;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use toolbox_server::lifecycle::{Health, LifecycleHandle, ReadinessCheck, Shutdown};

/// A check flipped by the test, shared with the handle via an `Arc` so both
/// sides see the same flag.
struct Toggle(Arc<AtomicBool>);

impl ReadinessCheck for Toggle {
    fn name(&self) -> &'static str {
        "toggle"
    }
    fn is_ready(&self) -> bool {
        self.0.load(Ordering::SeqCst)
    }
}

fn toggle(ready: bool) -> (Arc<AtomicBool>, Box<dyn ReadinessCheck>) {
    let flag = Arc::new(AtomicBool::new(ready));
    (Arc::clone(&flag), Box::new(Toggle(flag)))
}

#[test]
fn with_no_checks_a_fresh_handle_is_ready_at_once() {
    let handle = LifecycleHandle::new(Shutdown::new());
    assert_eq!(handle.current(), Health::Ready);
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
        Health::Ready,
        "passes once, so it latches"
    );
    flag.store(false, Ordering::SeqCst);
    assert_eq!(
        handle.current(),
        Health::Degraded,
        "was ready before, so a later failure is a regression, not a cold start"
    );
    flag.store(true, Ordering::SeqCst);
    assert_eq!(handle.current(), Health::Ready);
}

#[test]
fn shutdown_wins_over_every_check() {
    let shutdown = Shutdown::new();
    let handle = LifecycleHandle::new(shutdown.clone());
    assert_eq!(handle.current(), Health::Ready);

    shutdown.begin();
    assert_eq!(handle.current(), Health::ShuttingDown);
}

#[test]
fn is_ready_is_true_for_ready_alone() {
    assert!(Health::Ready.is_ready());
    assert!(!Health::Starting.is_ready());
    assert!(!Health::Degraded.is_ready());
    assert!(!Health::ShuttingDown.is_ready());
}
