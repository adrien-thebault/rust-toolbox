//! The drain sequence itself.
//!
//! It encodes the delay between failing readiness and refusing connections -
//! the step everyone omits, and the one that drops requests on every rolling
//! deploy.

use std::time::Duration;

#[cfg(unix)]
use tokio::signal::unix::{SignalKind, signal};
use tokio::sync::watch;
use tracing::{error, info};

/// How long to keep serving after readiness starts failing, and how long to
/// wait for in-flight requests once the listener is closed.
#[derive(Debug, Clone, Copy)]
pub struct ShutdownConfig {
    /// Time between `/ready` failing and the listener closing.
    ///
    /// This is the step that matters: a load balancer needs a few seconds to
    /// notice the failing probe and stop routing new requests. Closing the
    /// listener immediately drops whatever it sent in between.
    pub drain_delay: Duration,
    /// How long in-flight requests get to finish before the process exits.
    pub drain_timeout: Duration,
}

impl Default for ShutdownConfig {
    fn default() -> Self {
        Self {
            drain_delay: Duration::from_secs(5),
            drain_timeout: Duration::from_secs(30),
        }
    }
}

/// Resolves on `SIGTERM` or Ctrl-C.
///
/// `SIGTERM` is what a container runtime sends, so a server that only handles
/// Ctrl-C drains correctly in development and never in production.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        let _ = tokio::signal::ctrl_c().await;
    };

    #[cfg(unix)]
    let terminate = async {
        match signal(SignalKind::terminate()) {
            Ok(mut sig) => {
                sig.recv().await;
            }
            Err(e) => error!(error = %e, "cannot listen for SIGTERM"),
        }
    };
    // No SIGTERM off Unix: pending() leaves Ctrl-C as the only trigger and keeps
    // the select! below well-typed.
    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        () = ctrl_c => info!("received Ctrl-C, shutting down"),
        () = terminate => info!("received SIGTERM, shutting down"),
    }
}

/// A clonable handle to the process's drain state.
#[derive(Debug, Clone)]
pub struct Shutdown {
    /// Broadcasts the drain signal to every waiter.
    tx: watch::Sender<bool>,
}

impl Default for Shutdown {
    fn default() -> Self {
        Self::new()
    }
}

impl Shutdown {
    /// A handle that starts not shutting down.
    #[must_use]
    pub fn new() -> Self {
        Self {
            tx: watch::channel(false).0,
        }
    }

    /// A receiver that flips to `true` when shutdown begins.
    ///
    /// Long-lived streams - SSE, `WebSockets` - watch this and close themselves,
    /// since nothing else will interrupt them before `drain_timeout` expires.
    #[must_use]
    pub fn watch(&self) -> watch::Receiver<bool> {
        self.tx.subscribe()
    }

    /// Whether shutdown has begun.
    #[must_use]
    pub fn is_shutting_down(&self) -> bool {
        *self.tx.borrow()
    }

    /// Step 1: start failing readiness while continuing to serve.
    pub fn begin(&self) {
        // `send` fails and leaves the value untouched when nothing is
        // subscribed; `send_replace` always applies, so the flag is correct
        // whether or not anyone is watching.
        self.tx.send_replace(true);
    }

    /// Run steps 1 and 2, then resolve so the caller can stop accepting.
    ///
    /// Step 2 - the wait between failing readiness and closing the listener -
    /// is the whole point; see [`ShutdownConfig::drain_delay`].
    ///
    /// # Arguments
    ///
    /// * `cfg` - The timings. `drain_delay` is how long to keep serving on a
    ///   failing readiness probe, which is what gives the load balancer time to
    ///   notice.
    pub async fn drain(&self, cfg: ShutdownConfig) {
        self.begin();
        info!(
            delay_ms = u64::try_from(cfg.drain_delay.as_millis()).unwrap_or(u64::MAX),
            "readiness failing, waiting before closing the listener"
        );
        tokio::time::sleep(cfg.drain_delay).await;
    }
}
