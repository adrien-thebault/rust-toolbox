//! Bind, and carry the drain settings the serve loop needs.
//!
//! Binding a listener is the same handful of lines in every binary; carrying
//! the drain timings and handle in the same config keeps the serve loop from
//! re-deriving the step everyone omits, the one that drops requests on every
//! rolling deploy.

use std::net::SocketAddr;

use tracing::info;

use crate::lifecycle::{Shutdown, ShutdownConfig};

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

/// Bind, returning the listener.
///
/// The one step every transport shares. `toolbox-web` and `toolbox-grpc` each
/// wrap this with their own serve loop, because neither axum's nor tonic's
/// server type can be named here without depending on it.
///
/// # Arguments
///
/// * `cfg` - Where to listen.
///
/// # Errors
/// [`StartupError::Io`] when the address cannot be bound.
pub async fn bind(cfg: &StartupConfig) -> Result<tokio::net::TcpListener, StartupError> {
    let listener = tokio::net::TcpListener::bind(cfg.listen_addr).await?;
    info!(addr = %listener.local_addr()?, "listening");
    Ok(listener)
}
