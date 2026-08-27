//! The tower layer that scopes a trace context for the whole request.

use std::task::{Context, Poll};

use http::{HeaderValue, Request, Response};
use pin_project_lite::pin_project;
use tokio::task::futures::TaskLocalFuture;
use tower::{Layer, Service};

use super::{CURRENT_TRACE, TRACEPARENT, TraceContext, X_REQUEST_ID};

/// Extracts a `traceparent`, or mints one, and scopes it for the whole
/// request.
///
/// Always scoping - rather than only when a header was present - is what keeps
/// this to one future type and stops an `S::Future: Send + 'static` bound
/// leaking out to every caller.
#[derive(Debug, Clone, Copy, Default)]
pub struct TraceContextLayer;

impl TraceContextLayer {
    /// Build the layer.
    #[must_use]
    pub fn new() -> Self {
        Self
    }
}

impl<S> Layer<S> for TraceContextLayer {
    type Service = TraceContextService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        TraceContextService { inner }
    }
}

/// The service [`TraceContextLayer`] produces.
#[derive(Debug, Clone, Copy)]
pub struct TraceContextService<S> {
    inner: S,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for TraceContextService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = TraceContextFuture<S::Future>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        let ctx = req
            .headers()
            .get(TRACEPARENT)
            .and_then(|v| v.to_str().ok())
            .and_then(TraceContext::parse)
            .map_or_else(TraceContext::new_root, |parent| parent.child());

        let traceparent = HeaderValue::from_str(&ctx.to_string()).ok();
        let request_id = HeaderValue::from_str(ctx.request_id()).ok();
        req.extensions_mut().insert(ctx.clone());

        TraceContextFuture {
            inner: CURRENT_TRACE.scope(ctx, self.inner.call(req)),
            traceparent,
            request_id,
        }
    }
}

pin_project! {
    /// The future of [`TraceContextService`].
    ///
    /// Named rather than boxed: one `Box::pin` per request is an allocation on
    /// the hot path for no benefit.
    pub struct TraceContextFuture<F> {
        #[pin]
        inner: TaskLocalFuture<TraceContext, F>,
        traceparent: Option<HeaderValue>,
        request_id: Option<HeaderValue>,
    }
}

impl<F, ResBody, E> Future for TraceContextFuture<F>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
{
    type Output = Result<Response<ResBody>, E>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let mut res = std::task::ready!(this.inner.poll(cx))?;
        if let Some(v) = this.traceparent.take() {
            res.headers_mut().insert(TRACEPARENT, v);
        }
        if let Some(v) = this.request_id.take() {
            res.headers_mut().insert(X_REQUEST_ID, v);
        }
        Poll::Ready(Ok(res))
    }
}
