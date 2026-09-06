//! W3C Trace Context propagation.
//!
//! The `traceparent` header is a W3C recommendation that every tracing backend
//! already speaks, so the toolbox propagates that rather than the bespoke
//! `x-request-id` chain it used to. `x-request-id` survives only as a
//! human-quotable alias for the trace id.

use std::{
    fmt,
    task::{Context, Poll},
    time::Duration,
};

use http::{HeaderMap, HeaderName, HeaderValue, Request, Response};
use pin_project_lite::pin_project;
use tokio::task::futures::TaskLocalFuture;
use tower::{Layer, Service};
use tower_http::trace::{MakeSpan, OnEos, OnResponse};
use tracing::{Level, Span};

/// The W3C Trace Context header.
pub const TRACEPARENT: HeaderName = HeaderName::from_static("traceparent");

/// The conventional request-id header, kept as an alias so a human can quote
/// one short string.
pub const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// The `&str` form of [`X_REQUEST_ID`]. The suffix says what it is, rather
/// than the old pair where only the casing distinguished them.
pub const X_REQUEST_ID_NAME: &str = "x-request-id";

/// The only `traceparent` version this parses.
const VERSION: &str = "00";

/// The `sampled` flag bit.
const FLAG_SAMPLED: u8 = 0x01;

tokio::task_local! {
    /// The trace context of the request being handled.
    ///
    /// Always set inside a request handled through [`TraceContextLayer`]: the
    /// layer mints a context when the caller did not send one, so there is one
    /// branch rather than two.
    pub static CURRENT_TRACE: TraceContext;
}

/// A W3C `traceparent`: a trace id shared by every hop, a span id for this
/// hop, and the sampling flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceContext {
    /// Shared by every hop of the request.
    trace_id: String,
    /// Identifies this hop.
    span_id: String,
    /// W3C sampling flags.
    flags: u8,
}

impl TraceContext {
    /// Mint a new root context, sampled.
    #[must_use]
    pub fn new_root() -> Self {
        Self {
            trace_id: uuid::Uuid::now_v7().simple().to_string(),
            span_id: new_span_id(),
            flags: FLAG_SAMPLED,
        }
    }

    /// Parse a `traceparent` header value.
    ///
    /// Returns `None` for any value this does not fully understand, including
    /// the all-zero trace or span ids the specification forbids. The caller
    /// mints a fresh context in that case rather than propagating something
    /// invalid.
    ///
    /// # Arguments
    ///
    /// * `value` - A `traceparent` header value. Anything not fully understood
    ///   is `None`, so a malformed header mints a fresh trace instead of
    ///   propagating a broken one.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let mut parts = value.trim().split('-');
        let version = parts.next()?;
        let trace_id = parts.next()?;
        let span_id = parts.next()?;
        let flags = parts.next()?;
        if parts.next().is_some() || version != VERSION {
            return None;
        }
        if trace_id.len() != 32 || !is_lower_hex(trace_id) || trace_id.bytes().all(|b| b == b'0') {
            return None;
        }
        if span_id.len() != 16 || !is_lower_hex(span_id) || span_id.bytes().all(|b| b == b'0') {
            return None;
        }
        if flags.len() != 2 || !is_lower_hex(flags) {
            return None;
        }
        Some(Self {
            trace_id: trace_id.to_owned(),
            span_id: span_id.to_owned(),
            flags: u8::from_str_radix(flags, 16).ok()?,
        })
    }

    /// A child context: same trace, a fresh span id.
    #[must_use]
    pub fn child(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            span_id: new_span_id(),
            flags: self.flags,
        }
    }

    /// The trace id, shared by every hop of this request.
    #[must_use]
    pub fn trace_id(&self) -> &str {
        &self.trace_id
    }

    /// This hop's span id.
    #[must_use]
    pub fn span_id(&self) -> &str {
        &self.span_id
    }

    /// The trace id under the name a human will quote from an error page.
    #[must_use]
    pub fn request_id(&self) -> &str {
        &self.trace_id
    }

    /// Whether the caller asked for this trace to be recorded.
    #[must_use]
    pub fn is_sampled(&self) -> bool {
        self.flags & FLAG_SAMPLED != 0
    }
}

impl fmt::Display for TraceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{VERSION}-{}-{}-{:02x}",
            self.trace_id, self.span_id, self.flags
        )
    }
}

/// Whether a string is lowercase hexadecimal, which the specification requires:
/// an uppercase `traceparent` is invalid, not merely unusual.
///
/// # Arguments
///
/// * `s` - The trace or span id to check.
fn is_lower_hex(s: &str) -> bool {
    s.bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A 64-bit span id, distinct for every hop.
///
/// The **trailing** eight bytes of the UUID, not the leading ones: a v7 lays
/// its millisecond timestamp in the first six, so the head carries about
/// sixteen bits of entropy and a hundred spans in one millisecond would
/// collide roughly 7% of the time. The tail is `rand_b`, which is random.
fn new_span_id() -> String {
    let bytes = uuid::Uuid::now_v7().into_bytes();
    let tail: [u8; 8] = bytes[8..].try_into().unwrap_or([0; 8]);
    format!("{:016x}", u64::from_be_bytes(tail))
}

/// The request id of the request being handled, when there is one.
///
/// Returns `None` outside a request, which is what makes this usable from code
/// that also runs at startup or from a scheduled job.
#[must_use]
pub fn current_request_id() -> Option<String> {
    CURRENT_TRACE.try_with(|c| c.request_id().to_owned()).ok()
}

/// The trace context of the request being handled, when there is one.
#[must_use]
pub fn current_trace_context() -> Option<TraceContext> {
    CURRENT_TRACE.try_with(Clone::clone).ok()
}

/// Record the caller's resolved identity onto the current request span.
///
/// A no-op outside a request span. Called once identity is resolved -
/// `toolbox-web`'s session middleware, `toolbox-grpc`'s identity layer - so
/// every log line for the rest of the request, success or failure, names who
/// made it.
///
/// # Arguments
///
/// * `principal` - The resolved principal's subject.
pub fn record_principal(principal: &str) {
    Span::current().record("principal", principal);
}

/// The standard gRPC health-check method. A request to it is recorded at
/// `TRACE` rather than the configured level, since it is polled on a timer and
/// otherwise drowns real traffic.
const HEALTH_PROBE_PATH: &str = "/grpc.health.v1.Health/Check";

/// The header a `Trailers-Only` or completed gRPC response carries its real
/// outcome in - never the HTTP status, which stays 200 either way.
const GRPC_STATUS: &str = "grpc-status";

/// Whether these are a gRPC response's headers, by content type.
fn is_grpc(headers: &HeaderMap) -> bool {
    headers
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v.starts_with("application/grpc"))
}

/// The `status` [`OnResponse`] should record from a response's headers, or
/// `None` when the real outcome is not known yet.
///
/// A `Trailers-Only` gRPC failure carries `grpc-status` in these same headers,
/// so it is final already. A successful or streaming gRPC call has no
/// `grpc-status` here - only its trailers will - so recording the meaningless
/// HTTP 200 now would leave `status` recorded twice on the same span once
/// [`OnEos`] corrects it. Plain HTTP is never [`is_grpc`], so its status is
/// always final right here.
///
/// # Arguments
///
/// * `headers` - The response's headers.
/// * `http_status` - The response's HTTP status, used when this is not gRPC.
fn response_status(headers: &HeaderMap, http_status: u16) -> Option<String> {
    if let Some(status) = headers.get(GRPC_STATUS).and_then(|v| v.to_str().ok()) {
        Some(status.to_owned())
    } else if is_grpc(headers) {
        None
    } else {
        Some(http_status.to_string())
    }
}

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
    /// The wrapped service.
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

/// Builds one span per request carrying method, path and the W3C trace id.
///
/// It bridges `tower-http`'s tracing layer and this crate's trace context,
/// which do not know about each other. The trace id is the same string the
/// response's `x-request-id` carries, so a user quoting it from an error page
/// lands on exactly these log lines.
#[derive(Debug, Clone, Copy)]
pub struct MakeTracedSpan {
    /// The level the request span is recorded at.
    level: Level,
}

impl MakeTracedSpan {
    /// A span maker recording at `level`.
    ///
    /// # Arguments
    ///
    /// * `level` - What level to record request spans at. DEBUG for a chatty
    ///   internal service, INFO at an edge.
    #[must_use]
    pub fn new(level: Level) -> Self {
        Self { level }
    }
}

impl Default for MakeTracedSpan {
    fn default() -> Self {
        Self { level: Level::INFO }
    }
}

impl<B> MakeSpan<B> for MakeTracedSpan {
    fn make_span(&mut self, request: &Request<B>) -> Span {
        let trace_id = request
            .extensions()
            .get::<TraceContext>()
            .map_or_else(String::new, |c| c.trace_id().to_owned());

        macro_rules! span {
            ($level:expr) => {
                tracing::span!(
                    $level,
                    "request",
                    method = %request.method(),
                    path = %request.uri().path(),
                    trace_id = %trace_id,
                    status = tracing::field::Empty,
                    principal = tracing::field::Empty,
                    latency_ms = tracing::field::Empty,
                )
            };
        }

        // The gRPC health service is polled every few seconds - by an
        // orchestrator, and by `toolbox_grpc::client::poll_health` - so at an
        // edge's INFO level each probe would bury real traffic. The axum side
        // keeps `/ready` out of the stack entirely for the same reason; here
        // the span still exists, just below the default filter.
        let level = if request.uri().path() == HEALTH_PROBE_PATH {
            Level::TRACE
        } else {
            self.level
        };
        match level {
            Level::TRACE => span!(Level::TRACE),
            Level::DEBUG => span!(Level::DEBUG),
            Level::WARN => span!(Level::WARN),
            Level::ERROR => span!(Level::ERROR),
            Level::INFO => span!(Level::INFO),
        }
    }
}

/// Records the outcome, and the true latency, onto the request span.
///
/// `latency` is measured from a plain `Instant` by `tower_http`, independent of
/// how much the span was actually polled - unlike `tracing_subscriber`'s own
/// `time.busy`/`time.idle` (from `FmtSpan::CLOSE`), which count a `.await` on
/// real I/O (every DB-backed request here) as idle rather than busy, and so
/// understate it. Recorded unconditionally: it is known here even when
/// [`response_status`] is not yet (a gRPC call still awaiting its trailers),
/// and [`OnEos`] below only ever corrects `status`, never this.
impl<B> OnResponse<B> for MakeTracedSpan {
    fn on_response(self, response: &Response<B>, latency: Duration, span: &Span) {
        span.record("latency_ms", latency.as_millis());
        if let Some(status) = response_status(response.headers(), response.status().as_u16()) {
            span.record("status", status.as_str());
        }
    }
}

/// Corrects the span's status once a gRPC call's trailers arrive.
///
/// Only a gRPC response carries trailers at all - an HTTP response's
/// [`OnResponse`] already recorded the final status, so a missing
/// `grpc-status` here is left alone rather than overwritten with nothing.
impl OnEos for MakeTracedSpan {
    fn on_eos(self, trailers: Option<&HeaderMap>, _stream_duration: Duration, span: &Span) {
        if let Some(status) = trailers
            .and_then(|t| t.get(GRPC_STATUS))
            .and_then(|v| v.to_str().ok())
        {
            span.record("status", status);
        }
    }
}

// `response_status` and `is_grpc` are private, so an external `tests/` crate
// cannot reach them - these live here instead, the same exception
// `ClientInterceptor` documents in `toolbox-grpc`.
#[cfg(test)]
mod tests {
    use super::*;

    fn headers(pairs: &[(&str, &str)]) -> HeaderMap {
        let mut map = HeaderMap::new();
        for (name, value) in pairs {
            map.insert(
                HeaderName::from_bytes(name.as_bytes()).unwrap(),
                HeaderValue::from_str(value).unwrap(),
            );
        }
        map
    }

    #[test]
    fn a_plain_http_response_is_final_at_its_own_status() {
        assert_eq!(response_status(&headers(&[]), 500), Some("500".to_owned()));
    }

    #[test]
    fn a_trailers_only_grpc_failure_is_final_in_its_headers() {
        let h = headers(&[("content-type", "application/grpc"), ("grpc-status", "16")]);
        assert_eq!(response_status(&h, 200), Some("16".to_owned()));
    }

    #[test]
    fn a_streaming_grpc_response_with_no_grpc_status_yet_defers_to_on_eos() {
        let h = headers(&[("content-type", "application/grpc")]);
        assert_eq!(response_status(&h, 200), None);
    }
}
