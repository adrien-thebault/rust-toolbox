//! Serving a tonic server.
//!
//! Shaped to match `toolbox_web::server::serve`: assemble the transport object,
//! hand it over, and the one call binds, serves and drains. The one gRPC-only
//! difference is that the standard stack is applied here rather than by the
//! caller, because every service in one tonic server gets the same treatment -
//! there is no per-route split like the axum realtime one.

pub mod identity;
pub mod shared_secret;

use std::{sync::Arc, time::Duration};

pub use tonic::service::RoutesBuilder;
use tonic::transport::Server;
use toolbox_server::{
    shutdown::{ReadinessCheck, Shutdown, shutdown_signal},
    stack::{StackConfig, grpc_stack},
    startup::{StartupConfig, StartupError, bind},
};
use tracing::warn;

use crate::limits::MessageLimits;

/// How often the readiness poller re-evaluates the checks. It also re-evaluates
/// the instant shutdown begins, so this only bounds how stale a dependency
/// transition can be, not how fast the drain reacts.
const READINESS_POLL: Duration = Duration::from_secs(2);

/// What a gRPC server does beyond routing.
#[derive(Clone)]
pub struct ServerConfig {
    /// Message limits, as the single value both ends read.
    ///
    /// **Applied by you, on each service.** tonic puts
    /// `max_decoding_message_size` on the generated server type and there is no
    /// trait to reach it through, so this cannot be applied for you:
    ///
    /// ```ignore
    /// let cfg = ServerConfig::default();
    /// let mut routes = RoutesBuilder::default();
    /// routes.add_service(
    ///     TodoServiceServer::new(svc)
    ///         .max_decoding_message_size(cfg.limits.max_decoding)
    ///         .max_encoding_message_size(cfg.limits.max_encoding),
    /// );
    /// serve(serve_cfg, cfg, routes).await?;
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
    /// Whether to serve server reflection, so `grpcurl` works without the protos
    /// to hand.
    pub reflection: Option<&'static [u8]>,
    /// Dependencies the gRPC readiness probe consults, alongside the drain
    /// state. Empty means "ready whenever the process is up and not draining".
    pub readiness: Arc<Vec<Box<dyn ReadinessCheck>>>,
}

impl std::fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerConfig")
            .field("limits", &self.limits)
            .field("stack", &self.stack)
            .field("health", &self.health)
            .field("reflection", &self.reflection.is_some())
            .field("readiness", &self.readiness.len())
            .finish()
    }
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            limits: MessageLimits::default(),
            stack: StackConfig::default(),
            health: true,
            reflection: None,
            readiness: Arc::new(Vec::new()),
        }
    }
}

impl ServerConfig {
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

    /// Gate the gRPC readiness probe on `checks` as well as the drain state.
    ///
    /// # Arguments
    ///
    /// * `checks` - The dependencies readiness consults. The health probe for
    ///   the empty service name then reports `NOT_SERVING` while any of them is
    ///   unusable, the same signal the axum side's `/ready` gives.
    #[must_use]
    pub fn readiness_checks(mut self, checks: Vec<Box<dyn ReadinessCheck>>) -> Self {
        self.readiness = Arc::new(checks);
        self
    }
}

/// Bind, serve, and drain gracefully on `SIGTERM`.
///
/// The standard gRPC stack (`grpc_stack`) is applied for you, unlike the axum
/// side where `http_stack` is the caller's to place: a router with realtime
/// routes needs a different stack on those, whereas every service in one tonic
/// server gets the same treatment.
///
/// # Arguments
///
/// * `cfg` - Where to listen, plus the adapters and deployment the guard checks
///   first.
/// * `server` - What the server does beyond routing: the stack, health,
///   reflection, message limits and the readiness checks.
/// * `routes` - The services to serve. Health and reflection are added onto it
///   here, from `server`.
///
/// # Errors
/// [`StartupError::Deployment`] when a single-replica adapter is running
/// clustered, or [`StartupError::Io`] when the address cannot be bound.
pub async fn serve(
    cfg: StartupConfig<'_>,
    server: ServerConfig,
    mut routes: RoutesBuilder,
) -> Result<(), StartupError> {
    let health_reporter = if server.health {
        let (reporter, health) = tonic_health::server::health_reporter();
        // The empty service name is the gRPC convention for "the server as a
        // whole", which is what a Kubernetes gRPC probe with no `service`
        // checks.
        reporter
            .set_service_status("", tonic_health::ServingStatus::Serving)
            .await;
        routes.add_service(health);
        Some(reporter)
    } else {
        None
    };

    if let Some(descriptor) = server.reflection {
        match tonic_reflection::server::Builder::configure()
            .register_encoded_file_descriptor_set(descriptor)
            .build_v1()
        {
            Ok(service) => {
                routes.add_service(service);
            }
            Err(e) => warn!(error = %e, "reflection could not be enabled"),
        }
    }

    let listener = bind(&cfg).await?;
    let drain = cfg.shutdown;
    let shutdown = cfg.shutdown_handle.clone();

    // The health reporter goes to exactly one place: the poller when there are
    // checks to run, otherwise the shutdown future for a bare flip on `SIGTERM`.
    let (poll, drain_reporter) = match health_reporter {
        Some(reporter) if !server.readiness.is_empty() => (
            Some(poll_readiness(
                reporter,
                Arc::clone(&server.readiness),
                shutdown.clone(),
            )),
            None,
        ),
        other => (None, other),
    };

    let serve = Server::builder()
        .layer(grpc_stack(server.stack))
        .add_routes(routes.routes())
        .serve_with_incoming_shutdown(
            tokio_stream::wrappers::TcpListenerStream::new(listener),
            async move {
                shutdown_signal().await;
                // Fail the gRPC health probe the moment the signal lands, so a
                // Kubernetes readiness check pulls this replica before the drain
                // wait - the same contract the axum side's `/ready` honours.
                if let Some(reporter) = drain_reporter {
                    reporter
                        .set_service_status("", tonic_health::ServingStatus::NotServing)
                        .await;
                }
                shutdown.drain(drain).await;
            },
        );

    let result = if let Some(poll) = poll {
        tokio::select! {
            r = serve => r,
            () = poll => Ok(()),
        }
    } else {
        serve.await
    };
    result.map_err(|e| StartupError::Io(std::io::Error::other(e.to_string())))?;

    Ok(())
}

/// Keep the gRPC health status in step with the readiness checks until `serve`
/// returns.
///
/// tonic's health service reports the last status pushed to it, so unlike the
/// axum `/ready` route - which re-evaluates per request - this has to poll. It
/// wakes on the shutdown signal too, so a draining replica reports
/// `NOT_SERVING` at once rather than up to [`READINESS_POLL`] later.
async fn poll_readiness(
    reporter: tonic_health::server::HealthReporter,
    checks: Arc<Vec<Box<dyn ReadinessCheck>>>,
    shutdown: Shutdown,
) {
    let mut on_shutdown = shutdown.watch();
    let mut ticker = tokio::time::interval(READINESS_POLL);
    loop {
        let ready = !shutdown.is_shutting_down() && checks.iter().all(|check| check.is_ready());
        let status = if ready {
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
