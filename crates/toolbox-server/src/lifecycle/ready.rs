//! The readiness contract.

/// A dependency whose health decides whether this process should get traffic.
///
/// Here rather than in a transport crate so `toolbox-web`'s `/ready` route and
/// `toolbox-grpc`'s health poller read the one trait. Consulted on an ongoing
/// basis, not only at boot: a check that started passing can later fail, which
/// is what separates [`super::Health::Degraded`] from
/// [`super::Health::Starting`].
pub trait ReadinessCheck: Send + Sync + 'static {
    /// What to call it in a readiness report.
    fn name(&self) -> &'static str;
    /// Whether it is currently usable.
    fn is_ready(&self) -> bool;
}
