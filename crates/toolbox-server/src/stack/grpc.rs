//! The standard stack for a gRPC server.

use tower::{Layer, ServiceBuilder};
use tower_http::{
    catch_panic::{CatchPanic, CatchPanicLayer, DefaultResponseForPanic},
    classify::{GrpcErrorsAsFailures, SharedClassifier},
    trace::{Trace, TraceLayer},
};

use super::StackConfig;
use crate::{
    deadline::{DeadlineLayer, DeadlineService},
    span::MakeRequestSpan,
    trace_context::{TraceContextLayer, TraceContextService},
};

/// The service a gRPC stack wraps a server in.
pub type GrpcStacked<S> = CatchPanic<
    TraceContextService<
        Trace<DeadlineService<S>, SharedClassifier<GrpcErrorsAsFailures>, MakeRequestSpan>,
    >,
    DefaultResponseForPanic,
>;

/// The standard stack for a gRPC server.
///
/// Identical to [`HttpStack`](super::HttpStack) except that failures are
/// classified by `grpc-status` rather than by HTTP status, and an expired
/// deadline is answered as `grpc-status: 4` rather than as HTTP 504.
#[derive(Debug, Clone, Copy)]
pub struct GrpcStack {
    /// Timeouts, trace level and the rest, shared with the other stacks.
    cfg: StackConfig,
}

impl<S> Layer<S> for GrpcStack {
    type Service = GrpcStacked<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ServiceBuilder::new()
            .layer(CatchPanicLayer::new())
            .layer(TraceContextLayer::new())
            .layer(
                TraceLayer::new_for_grpc()
                    .make_span_with(MakeRequestSpan::new(self.cfg.trace_level)),
            )
            .layer(DeadlineLayer::new(self.cfg.timeout).grpc())
            .service(inner)
    }
}

/// The standard gRPC stack. See [`GrpcStack`].
///
/// # Arguments
///
/// * `cfg` - Timeout, body limit and trace level for this server.
#[must_use]
pub fn grpc_stack(cfg: StackConfig) -> GrpcStack {
    GrpcStack { cfg }
}
