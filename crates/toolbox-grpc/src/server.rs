//! Serving a tonic server.

pub mod identity;
pub mod shared_secret;

use tonic::{service::RoutesBuilder, transport::Server};
use toolbox_server::{
    shutdown::shutdown_signal,
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
    /// serve(serve_cfg, cfg.clone())
    ///     .add_service(
    ///         TodoServiceServer::new(svc)
    ///             .max_decoding_message_size(cfg.limits.max_decoding)
    ///             .max_encoding_message_size(cfg.limits.max_encoding),
    ///     )
    /// ```
    ///
    /// Carrying it here is still worth it: the client half reads the same value
    /// from `ClientChannel::limits()`, so the two ends drift only if somebody
    /// passes different configs, rather than by forgetting one.
    pub limits: MessageLimits,
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
}

/// Start building a gRPC server.
///
/// # Arguments
///
/// * `cfg` - Where to listen, plus the adapters and deployment the guard checks
///   first.
/// * `server` - What the server does beyond routing: health, reflection and
///   limits.
#[must_use]
pub fn serve(cfg: StartupConfig<'_>, server: ServerConfig) -> ServerBuilder<'_> {
    ServerBuilder {
        cfg,
        server,
        routes: RoutesBuilder::default(),
    }
}

/// Collects services, then serves them with the standard drain sequence.
pub struct ServerBuilder<'a> {
    /// Listen address, deployment and shutdown handle.
    cfg: StartupConfig<'a>,
    /// gRPC-specific stack and limit settings.
    server: ServerConfig,
    /// The services collected so far.
    routes: RoutesBuilder,
}

impl std::fmt::Debug for ServerBuilder<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ServerBuilder")
            .field("server", &self.server)
            .finish_non_exhaustive()
    }
}

impl ServerBuilder<'_> {
    /// Mount a service.
    ///
    /// # Arguments
    ///
    /// * `service` - A generated tonic service to mount. Every service added
    ///   here gets the same stack and the same drain.
    #[must_use]
    pub fn add_service<S>(mut self, service: S) -> Self
    where
        S: tower::Service<
                http::Request<tonic::body::Body>,
                Response = http::Response<tonic::body::Body>,
                Error = std::convert::Infallible,
            > + tonic::server::NamedService
            + Clone
            + Send
            + Sync
            + 'static,
        S::Future: Send + 'static,
    {
        self.routes.add_service(service);
        self
    }

    /// Bind and serve until `SIGTERM`, then drain.
    ///
    /// # Errors
    /// [`StartupError::Deployment`] when a single-replica adapter is running
    /// clustered, or [`StartupError::Io`] when the address cannot be bound.
    pub async fn run(mut self) -> Result<(), StartupError> {
        if self.server.health {
            let (reporter, health) = tonic_health::server::health_reporter();
            // The empty service name is the gRPC convention for "the server as
            // a whole", which is what a Kubernetes gRPC probe with no `service`
            // checks.
            reporter
                .set_service_status("", tonic_health::ServingStatus::Serving)
                .await;
            self.routes.add_service(health);
        }

        if let Some(descriptor) = self.server.reflection {
            match tonic_reflection::server::Builder::configure()
                .register_encoded_file_descriptor_set(descriptor)
                .build_v1()
            {
                Ok(service) => {
                    self.routes.add_service(service);
                }
                Err(e) => warn!(error = %e, "reflection could not be enabled"),
            }
        }

        let listener = bind(&self.cfg).await?;
        let shutdown = self.cfg.shutdown_handle.clone();
        let drain = self.cfg.shutdown;

        Server::builder()
            .add_routes(self.routes.routes())
            .serve_with_incoming_shutdown(
                tokio_stream::wrappers::TcpListenerStream::new(listener),
                async move {
                    shutdown_signal().await;
                    shutdown.drain(drain).await;
                },
            )
            .await
            .map_err(|e| StartupError::Io(std::io::Error::other(e.to_string())))?;

        Ok(())
    }
}
