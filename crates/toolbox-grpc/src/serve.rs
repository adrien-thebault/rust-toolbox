//! Serving a tonic server.

use tonic::{service::RoutesBuilder, transport::Server};
use toolbox_server::{
    serve::{ServeConfig, ServeError, bind},
    shutdown::shutdown_signal,
};
use tracing::warn;

use crate::backend::MessageLimits;

/// What a gRPC server does beyond routing.
#[derive(Debug, Clone)]
pub struct GrpcConfig {
    /// Message limits, as the single value both ends read.
    ///
    /// **Applied by you, on each service.** tonic puts
    /// `max_decoding_message_size` on the generated server type and there is no
    /// trait to reach it through, so this cannot be applied for you:
    ///
    /// ```ignore
    /// let cfg = GrpcConfig::default();
    /// serve_grpc(serve, cfg.clone())
    ///     .add_service(
    ///         TodoServiceServer::new(svc)
    ///             .max_decoding_message_size(cfg.limits.max_decoding)
    ///             .max_encoding_message_size(cfg.limits.max_encoding),
    ///     )
    /// ```
    ///
    /// Carrying it here is still worth it: the client half reads the same
    /// value from `BackendChannel::limits()`, so the two ends drift only if
    /// somebody passes different configs, rather than by forgetting one.
    pub limits: MessageLimits,
    /// Whether to serve the standard health service.
    pub health: bool,
    /// Whether to serve server reflection, so `grpcurl` works without the
    /// protos to hand.
    pub reflection: Option<&'static [u8]>,
}

impl Default for GrpcConfig {
    fn default() -> Self {
        Self {
            limits: MessageLimits::default(),
            health: true,
            reflection: None,
        }
    }
}

impl GrpcConfig {
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

/// Collects services, then serves them with the standard drain sequence.
pub struct GrpcServerBuilder<'a> {
    /// Listen address, deployment and shutdown handle.
    cfg: ServeConfig<'a>,
    /// gRPC-specific stack and limit settings.
    grpc: GrpcConfig,
    /// The services collected so far.
    routes: RoutesBuilder,
}

impl std::fmt::Debug for GrpcServerBuilder<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GrpcServerBuilder")
            .field("grpc", &self.grpc)
            .finish_non_exhaustive()
    }
}

/// Start building a gRPC server.
///
/// # Arguments
///
/// * `cfg` - Where to listen, plus the adapters and deployment the guard checks
///   first.
/// * `grpc` - What the server does beyond routing: health, reflection, limits
///   and service auth.
#[must_use]
pub fn serve_grpc(cfg: ServeConfig<'_>, grpc: GrpcConfig) -> GrpcServerBuilder<'_> {
    GrpcServerBuilder {
        cfg,
        grpc,
        routes: RoutesBuilder::default(),
    }
}

impl GrpcServerBuilder<'_> {
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
    /// [`ServeError::Deployment`] when a single-replica adapter is running
    /// clustered, or [`ServeError::Io`] when the address cannot be bound.
    pub async fn run(mut self) -> Result<(), ServeError> {
        if self.grpc.health {
            // Four lines that every gRPC service needs and neither template had.
            let (reporter, health) = tonic_health::server::health_reporter();
            reporter.set_serving::<HealthMarker>().await;
            self.routes.add_service(health);
        }

        if let Some(descriptor) = self.grpc.reflection {
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
            .map_err(|e| ServeError::Io(std::io::Error::other(e.to_string())))?;

        Ok(())
    }
}

/// The service name the health reporter marks serving.
///
/// The empty name is the convention for "the server as a whole", which is what
/// a Kubernetes gRPC probe checks.
#[derive(Debug, Clone, Copy)]
struct HealthMarker;

impl tonic::server::NamedService for HealthMarker {
    const NAME: &'static str = "";
}
