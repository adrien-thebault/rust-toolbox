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
pub mod startup;
pub mod telemetry;
pub mod trace_context;

pub use deadline::{DEADLINE, DeadlineLayer, current_deadline, time_remaining};
pub use lifecycle::{
    Health, LifecycleHandle, ReadinessCheck, Shutdown, ShutdownConfig, shutdown_signal,
    wait_until_ready,
};
pub use stack::{
    GrpcStack, HttpStack, RealtimeStack, StackConfig, grpc_stack, http_stack, realtime_stack,
};
pub use startup::{StartupConfig, StartupError, bind};
pub use telemetry::{LogFormat, TelemetryError, TelemetryGuard};
pub use trace_context::{
    CURRENT_TRACE, MakeTracedSpan, TRACEPARENT, TraceContext, TraceContextLayer, X_REQUEST_ID,
    X_REQUEST_ID_NAME, current_request_id, current_trace_context,
};
