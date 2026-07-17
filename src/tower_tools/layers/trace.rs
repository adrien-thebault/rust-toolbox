//! request tracing built on `tower-http`'s [`TraceLayer`]: [`MakeRequestSpan`]
//! builds each request's span (tagged with the request id set by
//! [`request_id_layer`](super::request_id::request_id_layer), if present),
//! and [`http_trace_layer`]/[`grpc_trace_layer`] configure the log levels.

use crate::tower_tools::layers::request_id::RequestId;
use http::Request;
use tower_http::classify::{GrpcErrorsAsFailures, ServerErrorsAsFailures, SharedClassifier};
use tower_http::trace::{
    DefaultOnFailure, DefaultOnRequest, DefaultOnResponse, MakeSpan, TraceLayer,
};
use tracing::{Level, Span};

/// builds each request's span, tagged with the request id set by
/// [`request_id_layer`](super::request_id::request_id_layer) if present
/// (`"-"` otherwise).
#[derive(Clone, Copy, Default)]
pub struct MakeRequestSpan;

impl<B> MakeSpan<B> for MakeRequestSpan {
    fn make_span(&mut self, request: &Request<B>) -> Span {
        let request_id = request
            .extensions()
            .get::<RequestId>()
            .and_then(|id| id.header_value().to_str().ok())
            .unwrap_or("-");
        tracing::info_span!(
            "request",
            rid = %request_id,
            method = %request.method(),
            path = %request.uri().path(),
        )
    }
}

/// the concrete [`TraceLayer`] type returned by [`http_trace_layer`]
pub type HttpTraceLayer = TraceLayer<SharedClassifier<ServerErrorsAsFailures>, MakeRequestSpan>;

/// a [`TraceLayer`] for a plain HTTP service: classifies 5xx responses as
/// failures (logged at `ERROR`), everything else at `INFO`.
pub fn http_trace_layer() -> HttpTraceLayer {
    TraceLayer::new_for_http()
        .make_span_with(MakeRequestSpan)
        .on_request(DefaultOnRequest::new().level(Level::INFO))
        .on_response(DefaultOnResponse::new().level(Level::INFO))
        .on_failure(DefaultOnFailure::new().level(Level::ERROR))
}

/// the concrete [`TraceLayer`] type returned by [`grpc_trace_layer`]
pub type GrpcTraceLayer = TraceLayer<SharedClassifier<GrpcErrorsAsFailures>, MakeRequestSpan>;

/// a [`TraceLayer`] for a tonic gRPC service: classifies by the `grpc-status`
/// trailer rather than the (almost always `200`) HTTP status code - see
/// [`GrpcErrorsAsFailures`].
pub fn grpc_trace_layer() -> GrpcTraceLayer {
    TraceLayer::new_for_grpc()
        .make_span_with(MakeRequestSpan)
        .on_request(DefaultOnRequest::new().level(Level::INFO))
        .on_response(DefaultOnResponse::new().level(Level::INFO))
        .on_failure(DefaultOnFailure::new().level(Level::ERROR))
}
