//! Check the deployment, bind, and carry the drain settings the serve loop
//! needs.
//!
//! Refusing to start on a deployment mismatch and binding a listener is the
//! same twenty lines in every binary; carrying the drain timings and handle in
//! the same config keeps the serve loop from re-deriving the step everyone
//! omits, the one that drops requests on every rolling deploy.

use std::net::SocketAddr;

use toolbox_cluster::{Adapter, Deployment, DeploymentError, check_deployment};
use tracing::info;

use crate::shutdown::{Shutdown, ShutdownConfig};

/// Everything `serve_*` needs that is not the application itself.
pub struct StartupConfig<'a> {
    /// Where to bind.
    pub listen_addr: SocketAddr,
    /// How many replicas are running.
    pub deployment: &'a Deployment,
    /// Every stateful adapter this process built.
    ///
    /// Passing them is slightly tedious and entirely honest. A global registry
    /// would be the alternative, and it would hide exactly the thing the guard
    /// exists to make visible.
    pub adapters: &'a [&'a dyn Adapter],
    /// Drain timings.
    pub shutdown: ShutdownConfig,
    /// The process's shutdown handle, so `/ready` and any long-lived stream
    /// share the one this server drains on.
    pub shutdown_handle: Shutdown,
}

impl<'a> StartupConfig<'a> {
    /// A config with the default drain timings and a fresh shutdown handle.
    ///
    /// # Arguments
    ///
    /// * `listen_addr` - Where to bind. Bind failures surface here rather than
    ///   deep inside the transport's own serve loop.
    /// * `deployment` - How many replicas are running, checked against the
    ///   adapters before the listener opens.
    #[must_use]
    pub fn new(listen_addr: SocketAddr, deployment: &'a Deployment) -> Self {
        Self {
            listen_addr,
            deployment,
            adapters: &[],
            shutdown: ShutdownConfig::default(),
            shutdown_handle: Shutdown::new(),
        }
    }

    /// Declare the stateful adapters this process built.
    ///
    /// # Arguments
    ///
    /// * `adapters` - Every stateful component this process wired up. This is
    ///   what the deployment guard reads, so an omitted adapter is an unchecked
    ///   one.
    #[must_use]
    pub fn adapters(mut self, adapters: &'a [&'a dyn Adapter]) -> Self {
        self.adapters = adapters;
        self
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
    /// A local adapter was running under `DEPLOYMENT=clustered`.
    #[error(transparent)]
    Deployment(#[from] DeploymentError),
    /// The listener could not be bound, or the server failed while running.
    #[error("server io: {0}")]
    Io(#[from] std::io::Error),
}

/// Check the deployment and bind, returning the listener.
///
/// The two steps every transport shares. `toolbox-web` and `toolbox-grpc` each
/// wrap this with their own serve loop, because neither axum's nor tonic's
/// server type can be named here without depending on it.
///
/// # Arguments
///
/// * `cfg` - Where to listen, plus the adapters and deployment the guard checks
///   first.
///
/// # Errors
/// [`StartupError::Deployment`] when a `Local` adapter is running clustered, or
/// [`StartupError::Io`] when the address cannot be bound.
pub async fn bind(cfg: &StartupConfig<'_>) -> Result<tokio::net::TcpListener, StartupError> {
    check_deployment(cfg.deployment, cfg.adapters)?;
    let listener = tokio::net::TcpListener::bind(cfg.listen_addr).await?;
    info!(
        addr = %listener.local_addr()?,
        deployment = ?cfg.deployment,
        adapters = cfg.adapters.len(),
        "listening"
    );
    Ok(listener)
}
