//! The state `health_router` reads: the drain's readiness flag, plus any extra
//! dependency checks `/ready` must also pass.

use std::sync::Arc;

pub use toolbox_server::shutdown::ReadinessCheck;
use toolbox_server::shutdown::ReadinessHandle;

/// The state `health_router` needs.
#[derive(Clone)]
pub struct HealthState {
    /// Whether the process is accepting traffic.
    pub(super) readiness: ReadinessHandle,
    /// Extra checks `/ready` must also pass.
    pub(super) checks: Arc<Vec<Box<dyn ReadinessCheck>>>,
}

impl std::fmt::Debug for HealthState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HealthState")
            .field("checks", &self.checks.len())
            .finish_non_exhaustive()
    }
}

impl HealthState {
    /// A state reporting ready until shutdown begins.
    ///
    /// # Arguments
    ///
    /// * `readiness` - The handle the drain flips, which is what makes `/ready`
    ///   fail before the listener closes.
    #[must_use]
    pub fn new(readiness: ReadinessHandle) -> Self {
        Self {
            readiness,
            checks: Arc::new(Vec::new()),
        }
    }

    /// Register the dependencies readiness depends on.
    ///
    /// # Arguments
    ///
    /// * `checks` - The dependencies readiness consults. Liveness never does,
    ///   because a database outage that fails liveness restarts every replica.
    #[must_use]
    pub fn with_checks(mut self, checks: Vec<Box<dyn ReadinessCheck>>) -> Self {
        self.checks = Arc::new(checks);
        self
    }
}
