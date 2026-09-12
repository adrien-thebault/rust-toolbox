//! Assembling a server and bringing it up: the bind, the background work and
//! the drain handle a transport's `serve` then runs.
//!
//! [`ServerBuilder`] gathers the bind address, the drain timings and the
//! background work ([`Probe`]s and plain tasks); [`build`](ServerBuilder::build)
//! binds the listener, spawns everything, and hands back a live [`Server`] that
//! `toolbox-grpc` and `toolbox-web` each drive. Three submodules own the
//! pieces: `shutdown` the drain sequence, `health` the dependency contract,
//! `lifecycle` where the two combine.

mod health;
mod lifecycle;
mod shutdown;

use std::{future::Future, net::SocketAddr, pin::Pin};

pub use health::{Health, HealthCheck, Probe, poll_check};
pub use lifecycle::{LifecycleHandle, wait_until_healthy};
pub use shutdown::{Shutdown, ShutdownConfig, shutdown_signal};
use tokio::net::TcpListener;
use tracing::info;

/// A boxed background future: the shape of a [`Probe`]'s polling loop and of
/// any extra task a [`Server`] carries.
pub type Task = Pin<Box<dyn Future<Output = ()> + Send>>;

/// Why a server failed to start or stopped.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ServerError {
    /// The listener could not be bound, or the server failed while running.
    #[error("server io: {0}")]
    Io(#[from] std::io::Error),
}

/// Gathers everything a transport `serve` needs that is not the application
/// itself: where to bind, how to drain, and the background work to run
/// alongside.
///
/// Transport-neutral on purpose. It owns no database, no `C` generic and no
/// handback closure - the caller builds its pool(s), services and scheduler
/// linearly and hands the leftovers (the readiness [`Probe`]s, a scheduler
/// [`task`](Self::task)) here. [`build`](Self::build) turns it into the live
/// [`Server`] that `toolbox_grpc::serve` and `toolbox_web::serve` take.
pub struct ServerBuilder {
    /// Where to bind.
    listen_addr: SocketAddr,
    /// Drain timings.
    shutdown: ShutdownConfig,
    /// The shared drain handle `/ready` and long-lived streams also watch.
    shutdown_handle: Shutdown,
    /// Readiness probes: each a health view plus its polling loop.
    probes: Vec<Probe>,
    /// Extra background tasks (a scheduler loop), aborted on drain.
    tasks: Vec<Task>,
}

impl std::fmt::Debug for ServerBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerBuilder")
            .field("listen_addr", &self.listen_addr)
            .field("shutdown", &self.shutdown)
            .field("probes", &self.probes.len())
            .field("tasks", &self.tasks.len())
            .finish_non_exhaustive()
    }
}

impl ServerBuilder {
    /// Start a builder that will bind `listen_addr`, with the default drain
    /// timings and a fresh shutdown handle.
    ///
    /// # Arguments
    ///
    /// * `listen_addr` - Where to bind. A bind failure surfaces from
    ///   [`build`](Self::build), not deep inside a transport's serve loop.
    #[must_use]
    pub fn listening_on(listen_addr: SocketAddr) -> Self {
        Self {
            listen_addr,
            shutdown: ShutdownConfig::default(),
            shutdown_handle: Shutdown::new(),
            probes: Vec::new(),
            tasks: Vec::new(),
        }
    }

    /// Keep serving for `delay` after `/ready` starts failing, so a load
    /// balancer has time to notice before the listener closes.
    ///
    /// # Arguments
    ///
    /// * `delay` - The gap between failing readiness and closing the listener.
    #[must_use]
    pub fn drain_after(mut self, delay: std::time::Duration) -> Self {
        self.shutdown.drain_delay = delay;
        self
    }

    /// Replace the whole drain configuration.
    ///
    /// # Arguments
    ///
    /// * `cfg` - Both timings at once. [`drain_after`](Self::drain_after) sets
    ///   the one that usually matters.
    #[must_use]
    pub fn shutdown_config(mut self, cfg: ShutdownConfig) -> Self {
        self.shutdown = cfg;
        self
    }

    /// Share an existing shutdown handle, so `/ready` and any long-lived
    /// stream drain on the same one this server does.
    ///
    /// # Arguments
    ///
    /// * `handle` - An existing handle, rather than the fresh one
    ///   [`listening_on`](Self::listening_on) made.
    #[must_use]
    pub fn shutdown_handle(mut self, handle: Shutdown) -> Self {
        self.shutdown_handle = handle;
        self
    }

    /// Register a readiness probe. Repeatable.
    ///
    /// [`build`](Self::build) spawns its polling loop and folds its health
    /// view into `/ready` (and the gRPC health service).
    ///
    /// # Arguments
    ///
    /// * `probe` - From [`poll_check`] or `toolbox_grpc::client::poll_health`.
    #[must_use]
    pub fn check(mut self, probe: Probe) -> Self {
        self.probes.push(probe);
        self
    }

    /// Register a background task to run for the life of the server. Repeatable.
    ///
    /// [`build`](Self::build) spawns it and the drain aborts it - which is what
    /// lets a scheduler loop (`Scheduler::into_task`) stop cleanly on `SIGTERM`
    /// rather than being left running by a detached `tokio::spawn`.
    ///
    /// # Arguments
    ///
    /// * `task` - The future to drive. It is not expected to return; if it
    ///   does, nothing observes the outcome.
    #[must_use]
    pub fn task(mut self, task: impl Future<Output = ()> + Send + 'static) -> Self {
        self.tasks.push(Box::pin(task));
        self
    }

    /// Bind the listener, spawn every probe loop and background task, and hand
    /// back the live [`Server`] a transport's `serve` drives.
    ///
    /// The one spawn point. Nothing runs until this is called, and it is
    /// called from within the tokio runtime immediately before `serve`; a
    /// bind failure surfaces here rather than mid-loop. If the bind fails
    /// nothing is spawned.
    ///
    /// # Errors
    /// [`ServerError::Io`] when the address cannot be bound.
    pub async fn build(self) -> Result<Server, ServerError> {
        let listener = TcpListener::bind(self.listen_addr).await?;
        info!(addr = %listener.local_addr()?, "listening");

        let mut checks: Vec<Box<dyn HealthCheck>> = Vec::with_capacity(self.probes.len());
        let mut handles = Vec::with_capacity(self.probes.len() + self.tasks.len());
        for probe in self.probes {
            checks.push(probe.check);
            handles.push(tokio::spawn(probe.poll));
        }
        for task in self.tasks {
            handles.push(tokio::spawn(task));
        }
        let lifecycle = LifecycleHandle::new(self.shutdown_handle.clone()).with_checks(checks);

        Ok(Server {
            listener,
            lifecycle,
            tasks: TaskGuard { handles },
            shutdown_handle: self.shutdown_handle,
            drain: self.shutdown,
        })
    }
}

/// A bound, running server: the input to `toolbox_grpc::serve` and
/// `toolbox_web::serve`.
///
/// Its probe loops and background tasks are already spawned; [`tasks`](Self::tasks)
/// aborts them on drop, so a `Server` that is built and then dropped rather
/// than served unwinds cleanly. The transport reads the fields directly.
#[derive(Debug)]
pub struct Server {
    /// The bound listener.
    pub listener: TcpListener,
    /// The combined readiness view, probe checks already registered - what
    /// `/ready` and the gRPC health poller read.
    pub lifecycle: LifecycleHandle,
    /// The spawned probe loops and extra tasks; aborted when dropped, or
    /// sooner with [`TaskGuard::abort`] (which `serve` does at drain start).
    pub tasks: TaskGuard,
    /// The shared drain handle, for `serve`'s shutdown future.
    pub shutdown_handle: Shutdown,
    /// The drain timings, for `serve`'s shutdown future.
    pub drain: ShutdownConfig,
}

/// Owns the [`JoinHandle`](tokio::task::JoinHandle)s of a server's spawned
/// background work and aborts them all when dropped.
///
/// A transport's `serve` aborts it the moment the drain begins - a scheduler
/// has no reason to keep ticking through a drain - and dropping it at the end
/// of `serve` is the backstop for the paths that return some other way.
#[derive(Debug)]
pub struct TaskGuard {
    /// One handle per spawned probe loop and per registered task.
    handles: Vec<tokio::task::JoinHandle<()>>,
}

impl TaskGuard {
    /// Abort every spawned task now, rather than waiting for drop. Idempotent.
    pub fn abort(&self) {
        for handle in &self.handles {
            handle.abort();
        }
    }
}

impl Drop for TaskGuard {
    fn drop(&mut self) {
        self.abort();
    }
}
