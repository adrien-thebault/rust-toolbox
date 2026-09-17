//! Serving a tonic server.
//!
//! Shaped to match `toolbox_web::serve`: build a [`Server`] with
//! `toolbox_server::ServerBuilder`, hand it over with a [`GrpcServerConfig`],
//! and the one call serves it and drains on `SIGTERM`. The bind and the
//! background work already happened in `ServerBuilder::build`. The one
//! gRPC-only difference is that the standard stack is applied here rather than
//! by the caller, because every service in one tonic server gets the same
//! treatment - there is no per-route split like the axum realtime one.

pub mod identity;
pub mod shared_secret;

use std::{fmt, io, time::Duration};

use secrecy::{ExposeSecret as _, SecretString};
use tokio::sync::oneshot;
use tokio_stream::wrappers::TcpListenerStream;
pub use tonic::service::{Routes, RoutesBuilder};
use tonic::transport::Server as TonicServer;
use toolbox_server::{
    LifecycleHandle, Server, ServerError,
    server::shutdown_signal,
    stack::{GrpcStack, StackConfig},
};
use tower::Layer as _;
use tracing::warn;

use crate::{limits::MessageLimits, shared_secret_layer};

/// How often the readiness poller re-evaluates the checks. It also re-evaluates
/// the instant shutdown begins, so this only bounds how stale a dependency
/// transition can be, not how fast the drain reacts.
const READINESS_POLL: Duration = Duration::from_secs(2);

/// What a gRPC server does beyond routing.
///
/// The readiness checks are not here: they ride on the [`Server`] as
/// [`Probe`](toolbox_server::Probe)s, the same as on the axum side, so a
/// process serving both transports registers each dependency once.
#[derive(Clone)]
pub struct GrpcServerConfig {
    /// Timeout, trace level and body limit for the standard stack this server
    /// wraps every service in.
    pub stack: StackConfig,
    /// Whether to serve the standard health service.
    pub health: bool,
    /// If set, the standard health service also requires this shared secret -
    /// so a caller who presents it proves the secret is correctly configured,
    /// not just that the process is up.
    ///
    /// `None` (the default) leaves health open, which is what a plain
    /// orchestrator probe with no way to attach a header needs. Only set this
    /// when the health service is consulted by another of your own services -
    /// never when it is also what your orchestrator polls directly, or that
    /// probe starts failing too.
    pub health_secret: Option<SecretString>,
    /// Whether to serve server reflection, so `grpcurl` works without the protos
    /// to hand.
    pub reflection: Option<&'static [u8]>,
}

impl fmt::Debug for GrpcServerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GrpcServerConfig")
            .field("stack", &self.stack)
            .field("health", &self.health)
            .field("health_secret", &self.health_secret.is_some())
            .field("reflection", &self.reflection.is_some())
            .finish()
    }
}

impl Default for GrpcServerConfig {
    fn default() -> Self {
        Self {
            stack: StackConfig::default(),
            health: true,
            health_secret: None,
            reflection: None,
        }
    }
}

impl GrpcServerConfig {
    /// The tonic limits derived from [`StackConfig::max_body_bytes`].
    ///
    /// Apply these to each generated server before moving this config into
    /// [`serve`]; tonic exposes message limits only on generated service types.
    #[must_use]
    pub fn message_limits(&self) -> MessageLimits {
        let max = self.stack.body_limit();
        MessageLimits {
            max_decoding: max,
            max_encoding: max,
        }
    }

    /// Serve reflection from a `tonic-build`-generated descriptor set.
    ///
    /// # Arguments
    ///
    /// * `descriptor` - The descriptor set `tonic-build` emitted. It is what
    ///   lets `grpcurl` call the service without a copy of the protos.
    #[must_use]
    pub fn reflection(mut self, descriptor: &'static [u8]) -> Self {
        self.reflection = Some(descriptor);
        self
    }

    /// Set the standard stack's timeout, body limit and trace level.
    ///
    /// # Arguments
    ///
    /// * `stack` - The stack settings. `StackConfig::unbounded()` is what a
    ///   server of long-lived streams needs, so the deadline layer does not cut
    ///   them off.
    #[must_use]
    pub fn stack(mut self, stack: StackConfig) -> Self {
        self.stack = stack;
        self
    }

    /// Require `secret` on the standard health service too. See
    /// [`GrpcServerConfig::health_secret`].
    ///
    /// # Arguments
    ///
    /// * `secret` - The shared secret a caller must present in
    ///   `x-shared-secret` to reach the health service.
    #[must_use]
    pub fn health_secret(mut self, secret: impl Into<String>) -> Self {
        self.health_secret = Some(SecretString::from(secret.into()));
        self
    }
}

/// Serve `server` and drain gracefully on `SIGTERM`.
///
/// The standard [`GrpcStack`] is applied for you, unlike the axum side where
/// the caller places each stack: a router with realtime
/// routes needs a different stack on those, whereas every service in one tonic
/// server gets the same treatment.
///
/// # Arguments
///
/// * `server` - The live [`Server`] from
///   [`ServerBuilder::build`](toolbox_server::ServerBuilder::build): a bound
///   listener, the spawned probe loops and tasks, and the drain handle. Its
///   probe checks gate the gRPC readiness status alongside the drain state.
/// * `config` - What the server does beyond routing: the stack, health,
///   reflection and message limits.
/// * `routes` - The services to serve, usually
///   `Routes::new(secret_layer.layer(svc.into_server()))` (chain
///   `.add_service` for a second domain in one process). Health and reflection
///   are added onto it here, from `config`.
///
/// # Errors
/// [`ServerError::Io`] when the server fails while running.
pub async fn serve(
    server: Server,
    config: GrpcServerConfig,
    mut routes: Routes,
) -> Result<(), ServerError> {
    let Server {
        listener,
        lifecycle,
        tasks,
        shutdown_handle,
        drain,
    } = server;

    let health_reporter = if config.health {
        let (reporter, health) = tonic_health::server::health_reporter();
        // The empty service name is the gRPC convention for "the server as a
        // whole", which is what a Kubernetes gRPC probe with no `service`
        // checks.
        reporter
            .set_service_status("", tonic_health::ServingStatus::Serving)
            .await;
        routes = if let Some(secret) = &config.health_secret {
            let layer = shared_secret_layer(secret.expose_secret())
                .map_err(|e| ServerError::Config(e.to_string()))?;
            routes.add_service(layer.layer(health))
        } else {
            routes.add_service(health)
        };
        Some(reporter)
    } else {
        None
    };

    if let Some(descriptor) = config.reflection {
        match tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(descriptor)
            .build_v1()
        {
            Ok(service) => {
                routes = routes.add_service(service);
            }
            Err(e) => warn!(error = %e, "reflection could not be enabled"),
        }
    }

    // The health reporter goes to exactly one place: the poller when there are
    // checks to run, otherwise the shutdown future for a bare flip on `SIGTERM`.
    let has_checks = !lifecycle.checks().is_empty();
    let (poll, drain_reporter) = match health_reporter {
        Some(reporter) if has_checks => (Some(poll_readiness(reporter, lifecycle)), None),
        other => (None, other),
    };

    let drain_timeout = drain.drain_timeout;
    let (draining_tx, draining_rx) = oneshot::channel();
    let serving = TonicServer::builder()
        .layer(GrpcStack::new(config.stack))
        .add_routes(routes)
        .serve_with_incoming_shutdown(TcpListenerStream::new(listener), async move {
            shutdown_signal().await;
            // A scheduler has no reason to keep ticking through a drain.
            tasks.abort();
            // Fail the gRPC health probe the moment the signal lands, so a
            // Kubernetes readiness check pulls this replica before the drain
            // wait - the same contract the axum side's `/ready` honours.
            if let Some(reporter) = drain_reporter {
                reporter
                    .set_service_status("", tonic_health::ServingStatus::NotServing)
                    .await;
            }
            shutdown_handle.drain(drain).await;
            let _ = draining_tx.send(());
        });

    // Wrap tonic's server so the in-flight timeout starts only after the
    // readiness delay has elapsed.
    let serve = async move {
        tokio::pin!(serving);
        // Wait for the first of the server and shutdown futures to finish.
        tokio::select! {
            // The server stopped by itself.
            result = &mut serving => result,
            // Shutdown finished its readiness delay; give in-flight requests
            // a bounded period to finish.
            _ = draining_rx => {
                if let Ok(result) = tokio::time::timeout(drain_timeout, &mut serving).await {
                    // The server drained before the deadline.
                    result
                } else {
                    // The drain deadline expired first.
                    warn!(timeout_ms = u64::try_from(drain_timeout.as_millis()).unwrap_or(u64::MAX),
                        "graceful shutdown timed out; terminating in-flight requests");
                    Ok(())
                }
            },
        }
    };

    let result = if let Some(poll) = poll {
        // The readiness poller is tied to this server and is dropped when the
        // serving future completes.
        tokio::select! {
            // Serving ended normally or through the bounded drain above.
            r = serve => r,
            // A readiness poller is normally endless; treat an unexpected
            // completion as a clean server stop.
            () = poll => Ok(()),
        }
    } else {
        serve.await
    };
    result.map_err(|e| ServerError::Io(io::Error::other(e.to_string())))?;

    Ok(())
}

/// Keep the gRPC health status in step with the lifecycle until `serve`
/// returns.
///
/// tonic's health service reports the last status pushed to it, so unlike the
/// axum `/ready` route - which re-evaluates per request - this has to poll. It
/// wakes on the shutdown signal too, so a draining replica reports
/// `NOT_SERVING` at once rather than up to [`READINESS_POLL`] later.
async fn poll_readiness(
    reporter: tonic_health::server::HealthReporter,
    lifecycle: LifecycleHandle,
) {
    let mut on_shutdown = lifecycle.shutdown_watch();
    let mut ticker = tokio::time::interval(READINESS_POLL);
    loop {
        let status = if lifecycle.current().is_healthy() {
            tonic_health::ServingStatus::Serving
        } else {
            tonic_health::ServingStatus::NotServing
        };
        reporter.set_service_status("", status).await;

        tokio::select! {
            _ = ticker.tick() => {}
            _ = on_shutdown.changed() => {}
        }
    }
}
