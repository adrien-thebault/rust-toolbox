use std::time::Duration;

use toolbox_server::lifecycle::{LifecycleHandle, Shutdown, wait_until_healthy};

#[tokio::test]
async fn a_handle_with_no_checks_resolves_at_once() {
    let handle = LifecycleHandle::new(Shutdown::new());
    tokio::time::timeout(Duration::from_millis(100), wait_until_healthy(&handle))
        .await
        .expect("no checks means healthy immediately");
}

#[tokio::test]
async fn shutdown_before_becoming_healthy_still_resolves_rather_than_hanging() {
    let shutdown = Shutdown::new();
    let handle = LifecycleHandle::new(shutdown.clone()).with_checks(vec![Box::new(NeverHealthy)]);
    shutdown.begin();

    tokio::time::timeout(Duration::from_millis(100), wait_until_healthy(&handle))
        .await
        .expect("a process asked to drain must not wait forever to become healthy");
}

/// A check that never passes.
struct NeverHealthy;

impl toolbox_server::lifecycle::HealthCheck for NeverHealthy {
    fn name(&self) -> &'static str {
        "never-healthy"
    }
    fn is_healthy(&self) -> bool {
        false
    }
}
