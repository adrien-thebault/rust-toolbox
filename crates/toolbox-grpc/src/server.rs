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

use std::time::Duration;

use secrecy::{ExposeSecret as _, SecretString};
use tokio_stream::wrappers::TcpListenerStream;
pub use tonic::service::{Routes, RoutesBuilder};
use tonic::transport::Server as TonicServer;
use toolbox_server::{
    LifecycleHandle, Server, ServerError,
    server::shutdown_signal,
    stack::{StackConfig, grpc_stack},
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
    /// Message limits, as the single value both ends read.
    ///
    /// **Applied by you, on each service.** tonic puts
    /// `max_decoding_message_size` on the generated server type and there is no
    /// trait to reach it through, so this cannot be applied for you:
    ///
    /// ```ignore
    /// let cfg = GrpcServerConfig::default();
    /// let routes = Routes::new(
    ///     TodoServiceServer::new(svc)
    ///         .max_decoding_message_size(cfg.limits.max_decoding)
    ///         .max_encoding_message_size(cfg.limits.max_encoding),
    /// );
    /// serve(server, cfg, routes).await?;
    /// ```
    ///
    /// Carrying it here is still worth it: the client half reads the same value
    /// from `ClientChannel::limits()`, so the two ends drift only if somebody
    /// passes different configs, rather than by forgetting one.
    pub limits: MessageLimits,
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

impl std::fmt::Debug for GrpcServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GrpcServerConfig")
            .field("limits", &self.limits)
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
            limits: MessageLimits::default(),
            stack: StackConfig::default(),
            health: true,
            health_secret: None,
            reflection: None,
        }
    }
}

impl GrpcServerConfig {
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
/// The standard gRPC stack (`grpc_stack`) is applied for you, unlike the axum
/// side where `http_stack` is the caller's to place: a router with realtime
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
            routes.add_service(shared_secret_layer(secret.expose_secret()).layer(health))
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

    let serve = TonicServer::builder()
        .layer(grpc_stack(config.stack))
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
        });

    let result = if let Some(poll) = poll {
        tokio::select! {
            r = serve => r,
            () = poll => Ok(()),
        }
    } else {
        serve.await
    };
    result.map_err(|e| ServerError::Io(std::io::Error::other(e.to_string())))?;

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
