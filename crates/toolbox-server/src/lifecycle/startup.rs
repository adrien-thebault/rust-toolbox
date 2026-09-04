//! The bind config, and waiting for the first readiness pass.

use std::{net::SocketAddr, time::Duration};

use super::{Health, LifecycleHandle, Shutdown, ShutdownConfig};

/// How often to re-check while waiting for first health.
const POLL: Duration = Duration::from_millis(200);

/// Everything `serve_*` needs that is not the application itself.
pub struct StartupConfig {
    /// Where to bind.
    pub listen_addr: SocketAddr,
    /// Drain timings.
    pub shutdown: ShutdownConfig,
    /// The process's shutdown handle, so `/ready` and any long-lived stream
    /// share the one this server drains on.
    pub shutdown_handle: Shutdown,
}

impl StartupConfig {
    /// A config with the default drain timings and a fresh shutdown handle.
    ///
    /// # Arguments
    ///
    /// * `listen_addr` - Where to bind. Bind failures surface here rather than
    ///   deep inside the transport's own serve loop.
    #[must_use]
    pub fn new(listen_addr: SocketAddr) -> Self {
        Self {
            listen_addr,
            shutdown: ShutdownConfig::default(),
            shutdown_handle: Shutdown::new(),
        }
    }

    /// Override the drain timings.
    ///
    /// # Arguments
    ///
    /// * `cfg` - The drain timings. The gap between failing readiness and
    ///   closing the listener is the one that matters.
    #[must_use]
    pub fn shutdown(mut self, cfg: ShutdownConfig) -> Self {
        self.shutdown = cfg;
        self
    }

    /// Share an existing shutdown handle, so `/ready` reports what this server
    /// is actually doing.
    ///
    /// # Arguments
    ///
    /// * `handle` - An existing handle, so `/ready` reports what this server is
    ///   actually doing rather than what a second handle believes.
    #[must_use]
    pub fn shutdown_handle(mut self, handle: Shutdown) -> Self {
        self.shutdown_handle = handle;
        self
    }
}

/// Why a server failed to start or stopped.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum StartupError {
    /// The listener could not be bound, or the server failed while running.
    #[error("server io: {0}")]
    Io(#[from] std::io::Error),
}

/// Wait until `handle` first reports [`Health::Healthy`], or shutdown begins
/// first.
///
/// For a `main` that wants to log `"started"`, or open some other startup
/// gate, only once the process can actually serve, rather than the instant
/// the listener binds.
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
