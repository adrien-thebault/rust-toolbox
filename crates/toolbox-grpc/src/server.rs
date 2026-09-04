//! Serving a tonic server.
//!
//! Shaped to match `toolbox_web::server::serve`: assemble the transport object,
//! hand it over, and the one call binds, serves and drains. The one gRPC-only
//! difference is that the standard stack is applied here rather than by the
//! caller, because every service in one tonic server gets the same treatment -
//! there is no per-route split like the axum realtime one.

pub mod identity;
pub mod shared_secret;

pub use tonic::service::RoutesBuilder;
use tonic::transport::Server;
use toolbox_server::{
    shutdown::shutdown_signal,
    stack::{StackConfig, grpc_stack},
    startup::{StartupConfig, StartupError, bind},
};
use tracing::warn;

use crate::limits::MessageLimits;

/// What a gRPC server does beyond routing.
#[derive(Debug, Clone)]
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
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            limits: MessageLimits::default(),
            stack: StackConfig::default(),
            health: true,
            reflection: None,
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
///   reflection and message limits.
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
    let shutdown = cfg.shutdown_handle.clone();
    let drain = cfg.shutdown;

    Server::builder()
        .layer(grpc_stack(server.stack))
        .add_routes(routes.routes())
        .serve_with_incoming_shutdown(
            tokio_stream::wrappers::TcpListenerStream::new(listener),
            async move {
                shutdown_signal().await;
                // Fail the gRPC health probe the moment the signal lands, so a
                // Kubernetes readiness check pulls this replica before the drain
                // wait - the same contract the axum side's `/ready` honours.
                if let Some(reporter) = health_reporter {
                    reporter
                        .set_service_status("", tonic_health::ServingStatus::NotServing)
                        .await;
                }
                shutdown.drain(drain).await;
            },
        )
        .await
        .map_err(|e| StartupError::Io(std::io::Error::other(e.to_string())))?;

    Ok(())
}
