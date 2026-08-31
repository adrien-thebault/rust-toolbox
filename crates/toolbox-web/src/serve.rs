//! Serving an axum router.
//!
//! `toolbox-server` owns the deployment check, the bind and the drain
//! sequence; this adds the axum-specific serve loop, because naming
//! `axum::Router` in `toolbox-server` would mean depending on axum there.

use std::net::SocketAddr;

use axum::Router;
use toolbox_server::{
    shutdown::shutdown_signal,
    startup::{StartupConfig, StartupError, bind},
};
use tracing::info;

/// Bind, serve, and drain gracefully on `SIGTERM`.
///
/// This does **not** apply `http_stack` for you. A router with realtime routes
/// needs `realtime_stack` on those and `http_stack` on the rest, and a function
/// that layers the whole router makes that impossible:
///
/// ```ignore
/// let app = rest.layer(http_stack(StackConfig::default()))
///     .merge(realtime.layer(realtime_stack()));
/// serve_http(cfg, app).await
/// ```
///
/// # Arguments
///
/// * `cfg` - Where to listen, plus the adapters and deployment the guard checks
///   first.
/// * `app` - The router, with its layers already applied. This does not apply
///   `http_stack` for you, because a router with realtime routes needs a
///   different stack on those.
///
/// # Errors
/// [`StartupError::Deployment`] when a single-replica adapter is running
/// clustered, or [`StartupError::Io`] when the address cannot be bound.
pub async fn serve_http(cfg: StartupConfig<'_>, app: Router) -> Result<(), StartupError> {
    let listener = bind(&cfg).await?;
    let shutdown = cfg.shutdown_handle.clone();
    let drain = cfg.shutdown;

    // ConnectInfo so `client_ip` has a peer address to fall back to.
    let service = app.into_make_service_with_connect_info::<SocketAddr>();

    axum::serve(listener, service)
        .with_graceful_shutdown(async move {
            shutdown_signal().await;
            // Step 1 and 2: fail readiness, keep serving for drain_delay so the
            // load balancer notices, and only then stop accepting.
            shutdown.drain(drain).await;
        })
        .await?;

    info!("listener closed, waiting for in-flight requests");
    Ok(())
}
