//! Boot, readiness and drain, as one state rather than three loosely related
//! flags.
//!
//! A process is starting, ready, degraded or draining, in that order except
//! that degraded can only follow ready. The three submodules each own one
//! mechanism - [`shutdown`] the drain sequence, [`ready`] the dependency
//! contract, [`startup`] waiting for the first pass - and this module is
//! where they combine into the one [`Health`] a caller actually wants to read.

pub mod ready;
pub mod shutdown;
pub mod startup;

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

pub use ready::ReadinessCheck;
pub use shutdown::{Shutdown, ShutdownConfig, shutdown_signal};
pub use startup::wait_until_ready;

/// Where the process is in its boot-to-drain lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// Up, but has not yet passed every readiness check once.
    Starting,
    /// Passing every readiness check, and not draining.
    Ready,
    /// Passed every readiness check before; at least one is failing now.
    ///
    /// A distinction [`Self::Starting`] cannot make on its own: a brand-new
    /// replica still warming up is not the same event as an established one
    /// whose dependency just died, and the two want different alerting.
    Degraded,
    /// The drain sequence has begun.
    ShuttingDown,
}

impl Health {
    /// Whether this process should receive new traffic.
    #[must_use]
    pub fn is_ready(&self) -> bool {
        matches!(self, Self::Ready)
    }
}

/// Reads the process's current [`Health`]: the drain state, plus every
/// registered [`ReadinessCheck`].
///
/// The one place this computation is written. `toolbox-web`'s `/ready` route
/// and `toolbox-grpc`'s health poller both read it rather than each
/// re-deriving `not draining && every check passes`.
#[derive(Clone)]
pub struct LifecycleHandle {
    /// The drain state.
    shutdown: Shutdown,
    /// Latched the first time every check has passed, so a later failure
    /// reads as [`Health::Degraded`] rather than [`Health::Starting`] again.
    ever_ready: Arc<AtomicBool>,
    /// The dependencies readiness consults.
    checks: Arc<Vec<Box<dyn ReadinessCheck>>>,
}

impl std::fmt::Debug for LifecycleHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LifecycleHandle")
            .field("checks", &self.checks.len())
            .finish_non_exhaustive()
    }
}

impl LifecycleHandle {
    /// A handle over `shutdown`, with no readiness checks yet.
    ///
    /// # Arguments
    ///
    /// * `shutdown` - The drain state to read. Share the same handle the
    ///   server drains on, so `/ready` reports what it is actually doing.
    #[must_use]
    pub fn new(shutdown: Shutdown) -> Self {
        Self {
            shutdown,
            ever_ready: Arc::new(AtomicBool::new(false)),
            checks: Arc::new(Vec::new()),
        }
    }

    /// Register the dependencies readiness consults.
    ///
    /// # Arguments
    ///
    /// * `checks` - The dependencies to consult. Liveness never sees these,
    ///   because a database outage that failed liveness would restart every
    ///   replica.
    #[must_use]
    pub fn with_checks(mut self, checks: Vec<Box<dyn ReadinessCheck>>) -> Self {
        self.checks = Arc::new(checks);
        self
    }

    /// The same, for a caller that already holds the checks behind an `Arc` -
    /// because its own config type needs to stay cheaply `Clone` despite the
    /// trait objects inside - and would otherwise have to unwrap it first.
    ///
    /// # Arguments
    ///
    /// * `checks` - The dependencies to consult, already shared.
    #[must_use]
    pub fn with_shared_checks(mut self, checks: Arc<Vec<Box<dyn ReadinessCheck>>>) -> Self {
        self.checks = checks;
        self
    }

    /// The registered checks, for a caller building its own report.
    #[must_use]
    pub fn checks(&self) -> &[Box<dyn ReadinessCheck>] {
        &self.checks
    }

    /// A receiver that flips to `true` when shutdown begins.
    ///
    /// For a poller that needs to wake immediately on the drain signal rather
    /// than wait for its next tick - checks have no async notification of
    /// their own, but the drain state does.
    #[must_use]
    pub fn shutdown_watch(&self) -> tokio::sync::watch::Receiver<bool> {
        self.shutdown.watch()
    }

    /// The current lifecycle state.
    #[must_use]
    pub fn current(&self) -> Health {
        if self.shutdown.is_shutting_down() {
            return Health::ShuttingDown;
        }
        if self.checks.iter().all(|c| c.is_ready()) {
            self.ever_ready.store(true, Ordering::SeqCst);
            return Health::Ready;
        }
        if self.ever_ready.load(Ordering::SeqCst) {
            Health::Degraded
        } else {
            Health::Starting
        }
    }
}
