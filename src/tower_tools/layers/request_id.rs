//! request-id assignment/propagation, built on `tower-http`'s own
//! [`SetRequestIdLayer`]/[`PropagateRequestIdLayer`]/[`MakeRequestUuid`].
//! Uses UUIDs rather than a counter, so ids stay unique across process
//! restarts and horizontally-scaled replicas.

use http::{HeaderName, Request};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
use tower::{Layer, Service};
pub use tower_http::request_id::{
    MakeRequestUuid, PropagateRequestIdLayer, RequestId, SetRequestIdLayer,
};

/// the name of the header that carries the request id across hops, for use in `http`/`tower`
pub const X_REQUEST_ID_STR: &str = "x-request-id";

/// [`X_REQUEST_ID_STR`] as an [`HeaderName`], for `tower`/`http`-typed APIs.
pub const X_REQUEST_ID: HeaderName = HeaderName::from_static(X_REQUEST_ID_STR);

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

tokio::task_local! {
    /// the current request's [`RequestId`], scoped by
    /// [`request_id_context_layer`] for the lifetime of handling one
    /// request.
    pub static CURRENT_REQUEST_ID: RequestId;
}

/// [`Layer`] for [`RequestIdContextService`] - see [`request_id_context_layer`].
#[derive(Debug, Clone, Copy, Default)]
pub struct RequestIdContextLayer;

impl<S> Layer<S> for RequestIdContextLayer {
    type Service = RequestIdContextService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RequestIdContextService { inner }
    }
}

/// wraps an inner service so that, for the lifetime of handling one
/// request, [`CURRENT_REQUEST_ID`] holds the [`RequestId`]
/// [`request_id_layer`] assigned to it - read off `extensions()`, so this
/// must be layered *inside* [`request_id_layer`] (same ordering
/// requirement as [`propagate_request_id_layer`]).
#[derive(Debug, Clone)]
pub struct RequestIdContextService<S> {
    inner: S,
}

impl<S, B> Service<Request<B>> for RequestIdContextService<S>
where
    S: Service<Request<B>>,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<B>) -> Self::Future {
        let id = req.extensions().get::<RequestId>().cloned();
        let fut = self.inner.call(req);
        Box::pin(async move {
            match id {
                Some(id) => CURRENT_REQUEST_ID.scope(id, fut).await,
                None => fut.await,
            }
        })
    }
}

/// wraps a service so that [`CURRENT_REQUEST_ID`] holds the current
/// request's [`RequestId`] for as long as it's being handled - see
/// [`RequestIdContextService`]. Must be layered *inside*
/// [`request_id_layer`] (it reads the id that layer assigned).
pub fn request_id_context_layer() -> RequestIdContextLayer {
    RequestIdContextLayer
}
