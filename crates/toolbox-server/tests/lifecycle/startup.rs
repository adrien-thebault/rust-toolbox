use std::time::Duration;

use toolbox_server::lifecycle::{LifecycleHandle, Shutdown, wait_until_ready};

#[tokio::test]
async fn a_handle_with_no_checks_resolves_at_once() {
    let handle = LifecycleHandle::new(Shutdown::new());
    tokio::time::timeout(Duration::from_millis(100), wait_until_ready(&handle))
        .await
        .expect("no checks means ready immediately");
}

#[tokio::test]
async fn shutdown_before_readiness_still_resolves_rather_than_hanging() {
    let shutdown = Shutdown::new();
    let handle = LifecycleHandle::new(shutdown.clone()).with_checks(vec![Box::new(NeverReady)]);
    shutdown.begin();

    tokio::time::timeout(Duration::from_millis(100), wait_until_ready(&handle))
        .await
        .expect("a process asked to drain must not wait forever to become ready");
}

/// A check that never passes.
struct NeverReady;

impl toolbox_server::lifecycle::ReadinessCheck for NeverReady {
    fn name(&self) -> &'static str {
        "never-ready"
    }
    fn is_ready(&self) -> bool {
        false
    }
}
