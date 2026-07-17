//! request-id assignment/propagation, built on `tower-http`'s own
//! [`SetRequestIdLayer`]/[`PropagateRequestIdLayer`]/[`MakeRequestUuid`].
//! Uses UUIDs rather than a counter, so ids stay unique across process
//! restarts and horizontally-scaled replicas.

use http::HeaderName;
pub use tower_http::request_id::{
    MakeRequestUuid, PropagateRequestIdLayer, RequestId, SetRequestIdLayer,
};

/// the header both layers below key off - set by [`request_id_layer`] on the
/// way in, echoed back onto the response by [`propagate_request_id_layer`].
pub const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// assigns a fresh [`RequestId`] (a UUID) to every request that doesn't
/// already carry one on `x-request-id`, stashing it in `extensions()` for
/// [`propagate_request_id_layer`] (and any other layer, e.g. a `TraceLayer`'s
/// `make_span_with`) to read.
pub fn request_id_layer() -> SetRequestIdLayer<MakeRequestUuid> {
    SetRequestIdLayer::new(X_REQUEST_ID, MakeRequestUuid)
}

/// echoes the request's `x-request-id` (set by [`request_id_layer`]) back
/// onto the response, so a client/proxy can correlate the two. Must be
/// layered *inside* [`request_id_layer`]: it reads the header off the
/// incoming request, so assignment has to have run first on the way in -
/// see the tests for the required ordering.
pub fn propagate_request_id_layer() -> PropagateRequestIdLayer {
    PropagateRequestIdLayer::new(X_REQUEST_ID)
}
