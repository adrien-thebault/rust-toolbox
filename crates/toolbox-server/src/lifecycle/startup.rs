//! Waiting for the first readiness pass.

use std::time::Duration;

use super::{Health, LifecycleHandle};

/// How often to re-check while waiting for first readiness.
const POLL: Duration = Duration::from_millis(200);

/// Wait until `handle` first reports [`Health::Ready`], or shutdown begins
/// first.
///
/// For a `main` that wants to log `"started"`, or open some other startup
/// gate, only once the process can actually serve, rather than the instant
/// the listener binds.
///
/// # Arguments
///
/// * `handle` - The lifecycle handle to poll.
pub async fn wait_until_ready(handle: &LifecycleHandle) {
    let mut ticker = tokio::time::interval(POLL);
    loop {
        match handle.current() {
            Health::Ready | Health::ShuttingDown => return,
            Health::Starting | Health::Degraded => {
                ticker.tick().await;
            }
        }
    }
}
