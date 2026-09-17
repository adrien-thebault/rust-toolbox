//! Serving an axum router.
//!
//! `toolbox-server` owns the bind, the drain sequence and the background work;
//! this adds the axum-specific serve loop, because naming `axum::Router` in
//! `toolbox-server` would mean depending on axum there.
//!
//! `serve` merges `/health` and `/ready` at the root itself, and - when the
//! [`WebServerConfig`] carries a spec - `/openapi.json` and `/docs` too, with
//! CORS as the outermost layer. It does **not** apply `apply_http_stack`: a
//! router with realtime routes needs `apply_realtime_stack` on those and
//! `apply_http_stack` on the rest, which a whole-router layer makes impossible.

use std::{fmt, future::IntoFuture as _, net::SocketAddr};

use axum::Router;
use tokio::sync::oneshot;
use toolbox_server::{Server, ServerError, server::shutdown_signal};
use tower_http::cors::CorsLayer;
use tracing::{info, warn};

use crate::{
    cors,
    health::{HealthState, health_router},
};

/// What an axum server does beyond its routes.
///
/// The health router is not optional - `serve` always merges `/health` and
/// `/ready` at the root - so it is absent here. CORS and the OpenAPI surface
/// are, because a service behind a same-origin proxy needs neither.
#[derive(Default)]
pub struct WebServerConfig {
    /// The CORS layer, applied as the outermost layer so a preflight to any
    /// path is answered even when the route itself is gated or gone. `None`
    /// adds no CORS.
    cors: Option<CorsLayer>,
    /// The assembled OpenAPI spec. `Some` merges `/openapi.json` and a Scalar
    /// `/docs` page at the root.
    #[cfg(feature = "openapi")]
    openapi: Option<utoipa::openapi::OpenApi>,
}

impl fmt::Debug for WebServerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("WebServerConfig");
        s.field("cors", &self.cors.is_some());
        #[cfg(feature = "openapi")]
        s.field("openapi", &self.openapi.is_some());
        s.finish()
    }
}

impl WebServerConfig {
    /// Allow exactly `origins`, with credentials.
    ///
    /// # Arguments
    ///
    /// * `origins` - The exact origins allowed. See [`cors::cors`].
    #[must_use]
    pub fn cors(mut self, origins: &[String]) -> Self {
        self.cors = Some(cors::cors(origins));
        self
    }

    /// Allow `origins` plus any `localhost`/loopback origin on any port.
    ///
    /// Dev only - see [`cors::cors_localhost`] for why that never ships.
    ///
    /// # Arguments
    ///
    /// * `origins` - The production origins to allow on top of the loopback
    ///   ones this reflects.
    #[must_use]
    pub fn cors_localhost(mut self, origins: &[String]) -> Self {
        self.cors = Some(cors::cors_localhost(origins));
        self
    }

    /// Use a hand-built CORS layer, for a policy neither helper covers.
    ///
    /// # Arguments
    ///
    /// * `layer` - The layer to apply outermost.
    #[must_use]
    pub fn cors_layer(mut self, layer: CorsLayer) -> Self {
        self.cors = Some(layer);
        self
    }

    /// Serve `spec` at `/openapi.json` and a Scalar docs page at `/docs`.
    ///
    /// # Arguments
    ///
    /// * `spec` - The assembled spec. Run
    ///   [`with_standard_errors`](crate::openapi::with_standard_errors) over it
    ///   first, or it will claim the endpoints cannot fail.
    #[cfg(feature = "openapi")]
    #[must_use]
    pub fn openapi(mut self, spec: utoipa::openapi::OpenApi) -> Self {
        self.openapi = Some(spec);
        self
    }
}

/// Serve `server` and drain gracefully on `SIGTERM`.
///
/// # Arguments
///
/// * `server` - The live [`Server`] from
///   [`ServerBuilder::build`](toolbox_server::ServerBuilder::build): a bound
///   listener, the spawned probe loops and tasks, and the drain handle. Its
///   probe checks are what `/ready` reports on.
/// * `config` - CORS and, optionally, the OpenAPI surface.
/// * `app` - The router, with its own layers (`apply_http_stack`,
///   `apply_realtime_stack`) already applied. `/health`, `/ready` and the
///   OpenAPI routes are merged onto it here.
///
/// # Errors
/// [`ServerError::Io`] when the server fails while running.
pub async fn serve(
    server: Server,
    config: WebServerConfig,
    app: Router,
) -> Result<(), ServerError> {
    let Server {
        listener,
        lifecycle,
        tasks,
        shutdown_handle,
        drain,
    } = server;

    // Health at the root, always: liveness/readiness and the drain contract
    // are not something a gateway opts out of. Merged after the caller's `app`,
    // so their HTTP stack never wraps it - otherwise the trace layer would log
    // the drain 503s `/ready` returns as errors on every rolling deploy.
    // CORS (below) is the one thing layered over health, so preflights are
    // still answered.
    #[cfg_attr(not(feature = "openapi"), allow(unused_mut))]
    let mut app = app.merge(health_router().with_state(HealthState::from_lifecycle(lifecycle)));

    #[cfg(feature = "openapi")]
    if let Some(spec) = config.openapi {
        use crate::{OpenApiConfig, openapi_router};
        app = app.merge(openapi_router(spec, &OpenApiConfig::default()));
    }

    // CORS outermost on purpose: a preflight to a path that is gated or gone
    // still has to be answered, which it would not be from inside the router.
    let app = match config.cors {
        Some(layer) => app.layer(layer),
        None => app,
    };

    // ConnectInfo so `client_ip` has a peer address to fall back to.
    let service = app.into_make_service_with_connect_info::<SocketAddr>();

    let drain_timeout = drain.drain_timeout;
    let (draining_tx, draining_rx) = oneshot::channel();
    let serving = axum::serve(listener, service)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            // A scheduler has no reason to keep ticking through a drain.
            tasks.abort();
            // Fail readiness, keep serving for drain_delay so the load
            // balancer notices, and only then stop accepting.
            shutdown_handle.drain(drain).await;
            let _ = draining_tx.send(());
        })
        .into_future();
    tokio::pin!(serving);

    // Wait for the first of the server and shutdown futures to finish.
    let result = tokio::select! {
        // The server stopped by itself.
        result = &mut serving => result,
        // Shutdown finished its readiness delay; give in-flight requests a
        // bounded period to finish.
        _ = draining_rx => {
            if let Ok(result) = tokio::time::timeout(drain_timeout, &mut serving).await {
                // The server drained before the deadline.
                result
            } else {
                // The drain deadline expired first.
                warn!(timeout_ms = u64::try_from(drain_timeout.as_millis()).unwrap_or(u64::MAX),
                    "graceful shutdown timed out; terminating in-flight requests");
                return Ok(());
            }
        },
    };
    result?;

    info!("listener closed, waiting for in-flight requests");
    Ok(())
}
