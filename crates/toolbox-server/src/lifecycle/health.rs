//! The health contract and the state it computes.

/// A dependency whose health decides whether this process should get traffic.
///
/// Here rather than in a transport crate so `toolbox-web`'s `/ready` route and
/// `toolbox-grpc`'s health poller read the one trait. Consulted on an ongoing
/// basis, not only at boot: a check that started passing can later fail, which
/// is what separates [`Health::Degraded`] from [`Health::Starting`].
pub trait HealthCheck: Send + Sync + 'static {
    /// What to call it in a health report.
    fn name(&self) -> &'static str;
    /// Whether it is currently usable.
    fn is_healthy(&self) -> bool;
}

/// Where the process is in its boot-to-drain lifecycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Health {
    /// Up, but has not yet passed every check once.
    Starting,
    /// Passing every check, and not draining.
    Healthy,
    /// Passed every check before; at least one is failing now.
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
    pub fn is_healthy(&self) -> bool {
        matches!(self, Self::Healthy)
    }
}
