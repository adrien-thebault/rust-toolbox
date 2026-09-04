//! The runtime half of the toolbox: everything an HTTP or gRPC process needs
//! that is not specific to either.
//!
//! It bridges `tower-http`, `tracing` and the two transport crates, which do
//! not know about each other. `toolbox-web` and `toolbox-grpc` both depend on
//! it; neither owns it, and it depends on neither axum nor tonic.

pub mod args;
pub mod deadline;
pub mod lifecycle;
pub mod stack;
pub mod telemetry;
pub mod trace_context;

pub use deadline::{DEADLINE, DeadlineLayer, current_deadline, time_remaining};
pub use lifecycle::{
    Health, HealthCheck, LifecycleHandle, Shutdown, ShutdownConfig, StartupConfig, StartupError,
    shutdown_signal, wait_until_healthy,
};
pub use stack::{
    GrpcStack, HttpStack, RealtimeStack, StackConfig, grpc_stack, http_stack, realtime_stack,
};
pub use telemetry::{LogFormat, TelemetryError, TelemetryGuard};
pub use trace_context::{
    CURRENT_TRACE, MakeTracedSpan, TRACEPARENT, TraceContext, TraceContextLayer, X_REQUEST_ID,
    X_REQUEST_ID_NAME, current_request_id, current_trace_context,
};
use tracing::info;

/// Bind, returning the listener.
///
/// The one step every transport shares. `toolbox-web` and `toolbox-grpc` each
/// wrap this with their own serve loop, because neither axum's nor tonic's
/// server type can be named here without depending on it.
///
/// # Arguments
///
/// * `cfg` - Where to listen.
///
/// # Errors
/// [`StartupError::Io`] when the address cannot be bound.
pub async fn bind(cfg: &StartupConfig) -> Result<tokio::net::TcpListener, StartupError> {
    let listener = tokio::net::TcpListener::bind(cfg.listen_addr).await?;
    info!(addr = %listener.local_addr()?, "listening");
    Ok(listener)
}
