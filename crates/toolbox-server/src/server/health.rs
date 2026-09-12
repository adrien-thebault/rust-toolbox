//! The health contract and the readiness probe.

use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use tracing::{info, warn};

use super::Task;

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

/// A [`HealthCheck`] whose answer is refreshed by a background loop rather than
/// computed inline.
///
/// `is_healthy` is deliberately synchronous - `/ready` and the gRPC readiness
/// poller both read it per call - so a check backed by a live network call
/// cannot answer inline without coupling their latency to the dependency's,
/// and without letting every independent reader trigger its own call against
/// it. The loop returned alongside it in a [`Probe`] runs the live call on its
/// own schedule and leaves this holding only the last result.
struct PolledCheck {
    /// What to call it in a health report.
    name: &'static str,
    /// The last result the polling loop stored.
    healthy: Arc<AtomicBool>,
}

impl HealthCheck for PolledCheck {
    fn name(&self) -> &'static str {
        self.name
    }

    fn is_healthy(&self) -> bool {
        self.healthy.load(Ordering::Relaxed)
    }
}

/// A [`HealthCheck`] paired with the loop that keeps it current.
///
/// [`poll_check`] and `toolbox_grpc::client::poll_health` both return one.
/// Hand it to [`ServerBuilder::check`](super::ServerBuilder::check); `build`
/// spawns `poll` and registers `check` with the readiness state, so the two
/// halves are never wired up by hand - the mistake the old "return a tuple,
/// remember to spawn the second half" shape invited.
pub struct Probe {
    /// The synchronous health view `/ready` and the gRPC poller read.
    pub check: Box<dyn HealthCheck>,
    /// The loop that refreshes `check`. Spawned by `build`, never by the
    /// caller.
    pub poll: Task,
}

impl std::fmt::Debug for Probe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Probe")
            .field("check", &self.check.name())
            .finish_non_exhaustive()
    }
}

/// Poll `probe(state.clone())` every `interval`, as a [`Probe`].
///
/// `state` is threaded through and cloned once per tick, rather than baked
/// into `probe` by the caller, so `probe` itself can stay a one-liner - a
/// gRPC channel or a `Db` handle cloned once up front, not once per call site
/// that wants a readiness probe.
///
/// The loop never returns; `build` spawns it. It logs only when the result
/// changes, including the first result - INFO on recovery, WARN on failure -
/// never once per tick, which is what makes polling a dependency this way
/// safe to run as often as every few seconds without flooding the log.
///
/// # Arguments
///
/// * `name` - Reported in the health response and in the transition log.
/// * `interval` - How often `probe` runs.
/// * `state` - Cloned once per tick and handed to `probe`. A gRPC
///   `ClientChannel` or a `Db<C>` are both cheap to clone - both wrap an
///   `Arc`-backed resource underneath.
/// * `probe` - The check itself. `true` means healthy.
pub fn poll_check<T, F, Fut>(name: &'static str, interval: Duration, state: T, probe: F) -> Probe
where
    T: Clone + Send + 'static,
    F: Fn(T) -> Fut + Send + Sync + 'static,
    Fut: Future<Output = bool> + Send,
{
    let healthy = Arc::new(AtomicBool::new(false));
    let check = PolledCheck {
        name,
        healthy: Arc::clone(&healthy),
    };
    let poll = async move {
        let mut ticker = tokio::time::interval(interval);
        let mut last: Option<bool> = None;
        loop {
            ticker.tick().await;
            let ok = probe(state.clone()).await;
            healthy.store(ok, Ordering::Relaxed);
            if last != Some(ok) {
                if ok {
                    info!(check = name, "dependency check recovered");
                } else {
                    warn!(check = name, "dependency check failed");
                }
                last = Some(ok);
            }
        }
    };
    Probe {
        check: Box::new(check),
        poll: Box::pin(poll),
    }
}
