//! Boot, health and drain combined into the one reading a caller wants.
//!
//! [`shutdown`](super::shutdown) owns the drain sequence and
//! [`health`](super::health) the dependency contract; this is where they
//! combine into [`LifecycleHandle`], which `toolbox-web`'s `/ready` route and
//! `toolbox-grpc`'s health poller both read rather than each re-deriving
//! `not draining && every check passes`.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use tokio::sync::watch;

use super::{Health, HealthCheck, Shutdown};

/// How often to re-check while [`wait_until_healthy`] waits for first health.
const POLL: Duration = Duration::from_millis(200);

/// Reads the process's current [`Health`]: the drain state, plus every
/// registered [`HealthCheck`].
#[derive(Clone)]
pub struct LifecycleHandle {
    /// The drain state.
    shutdown: Shutdown,
    /// Latched the first time every check has passed, so a later failure
    /// reads as [`Health::Degraded`] rather than [`Health::Starting`] again.
    ever_healthy: Arc<AtomicBool>,
    /// The dependencies health consults.
    checks: Arc<Vec<Box<dyn HealthCheck>>>,
}

impl std::fmt::Debug for LifecycleHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LifecycleHandle")
            .field("checks", &self.checks.len())
            .finish_non_exhaustive()
    }
}

impl LifecycleHandle {
    /// A handle over `shutdown`, with no health checks yet.
    ///
    /// # Arguments
    ///
    /// * `shutdown` - The drain state to read. Share the same handle the
    ///   server drains on, so `/ready` reports what it is actually doing.
    #[must_use]
    pub fn new(shutdown: Shutdown) -> Self {
        Self {
            shutdown,
            ever_healthy: Arc::new(AtomicBool::new(false)),
            checks: Arc::new(Vec::new()),
        }
    }

    /// Register the dependencies health consults.
    ///
    /// # Arguments
    ///
    /// * `checks` - The dependencies to consult. Liveness never sees these,
    ///   because a database outage that failed liveness would restart every
    ///   replica.
    #[must_use]
    pub fn with_checks(mut self, checks: Vec<Box<dyn HealthCheck>>) -> Self {
        self.checks = Arc::new(checks);
        self
    }

    /// The same, for a caller that already holds the checks behind an `Arc`.
    ///
    /// # Arguments
    ///
    /// * `checks` - The dependencies to consult, already shared.
    #[must_use]
    pub fn with_shared_checks(mut self, checks: Arc<Vec<Box<dyn HealthCheck>>>) -> Self {
        self.checks = checks;
        self
    }

    /// The registered checks, for a caller building its own report.
    #[must_use]
    pub fn checks(&self) -> &[Box<dyn HealthCheck>] {
        &self.checks
    }

    /// A receiver that flips to `true` when shutdown begins.
    ///
    /// For a poller that needs to wake immediately on the drain signal rather
    /// than wait for its next tick - checks have no async notification of
    /// their own, but the drain state does.
    #[must_use]
    pub fn shutdown_watch(&self) -> watch::Receiver<bool> {
        self.shutdown.watch()
    }

    /// The current lifecycle state.
    #[must_use]
    pub fn current(&self) -> Health {
        if self.shutdown.is_shutting_down() {
            return Health::ShuttingDown;
        }
        if self.checks.iter().all(|c| c.is_healthy()) {
            self.ever_healthy.store(true, Ordering::SeqCst);
            return Health::Healthy;
        }
        if self.ever_healthy.load(Ordering::SeqCst) {
            Health::Degraded
        } else {
            Health::Starting
        }
    }
}

/// Wait until `handle` first reports [`Health::Healthy`], or shutdown begins
/// first.
///
/// Opt-in, for a `main` that wants to log `"started"` or open some other
/// startup gate only once the process can actually serve rather than the
/// instant the listener binds. Never on the `serve` path: two interdependent
/// services that each waited for the other to be healthy would deadlock at
/// boot.
///
/// # Arguments
///
/// * `handle` - The lifecycle handle to poll.
pub async fn wait_until_healthy(handle: &LifecycleHandle) {
    let mut ticker = tokio::time::interval(POLL);
    loop {
        match handle.current() {
            Health::Healthy | Health::ShuttingDown => return,
            Health::Starting | Health::Degraded => {
                ticker.tick().await;
            }
        }
    }
}
