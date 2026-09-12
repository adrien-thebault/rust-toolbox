//! The state `health_router` reads: the process lifecycle, plus any extra
//! dependency checks `/ready` must also pass.

pub use toolbox_server::HealthCheck;
use toolbox_server::{LifecycleHandle, Shutdown};

/// The state `health_router` needs.
#[derive(Clone)]
pub struct HealthState {
    /// The drain state and the registered readiness checks, combined.
    pub(super) lifecycle: LifecycleHandle,
}

impl std::fmt::Debug for HealthState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HealthState")
            .field("lifecycle", &self.lifecycle)
            .finish()
    }
}

impl HealthState {
    /// A state reporting ready until shutdown begins.
    ///
    /// # Arguments
    ///
    /// * `shutdown` - The handle the drain flips, which is what makes `/ready`
    ///   fail before the listener closes. Share the same handle the server
    ///   drains on.
    #[must_use]
    pub fn new(shutdown: Shutdown) -> Self {
        Self {
            lifecycle: LifecycleHandle::new(shutdown),
        }
    }

    /// Register the dependencies readiness depends on.
    ///
    /// # Arguments
    ///
    /// * `checks` - The dependencies readiness consults. Liveness never does,
    ///   because a database outage that fails liveness restarts every replica.
    #[must_use]
    pub fn with_checks(mut self, checks: Vec<Box<dyn HealthCheck>>) -> Self {
        self.lifecycle = self.lifecycle.with_checks(checks);
        self
    }

    /// Wrap a [`LifecycleHandle`] a [`Server`](toolbox_server::Server) already
    /// assembled - its probe checks registered - so `/ready` reads the exact
    /// view the rest of the server drains on. This is what
    /// [`toolbox_web::serve`](crate::serve) uses; a hand-rolled server uses
    /// [`new`](Self::new) plus [`with_checks`](Self::with_checks).
    ///
    /// # Arguments
    ///
    /// * `lifecycle` - The combined drain-plus-checks handle, i.e.
    ///   [`Server::lifecycle`](toolbox_server::Server).
    #[must_use]
    pub fn from_lifecycle(lifecycle: LifecycleHandle) -> Self {
        Self { lifecycle }
    }
}
