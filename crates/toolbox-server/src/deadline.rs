//! Request deadlines, propagated the way gRPC already does it.
//!
//! It encodes the decision that a deadline is absolute and travels with the
//! request, so a gateway timeout stops the backends working on a request nobody
//! is waiting for any more.

use std::{
    task::{Context, Poll},
    time::{Duration, Instant},
};

use http::{HeaderName, HeaderValue, Request, Response, StatusCode};
use pin_project_lite::pin_project;
use tokio::{task::futures::TaskLocalFuture, time::Sleep};
use tower::{Layer, Service};

/// The gRPC deadline header, which this reads on the way in and
/// `toolbox-grpc` writes on the way out.
pub const GRPC_TIMEOUT: HeaderName = HeaderName::from_static("grpc-timeout");

tokio::task_local! {
    /// When the request being handled must be finished.
    ///
    /// Set whenever a deadline applies. Code that fans out to a backend reads
    /// it to decide how long the call downstream may take.
    pub static DEADLINE: Instant;
}

/// How long the request being handled has left, when it has a deadline.
#[must_use]
pub fn time_remaining() -> Option<Duration> {
    DEADLINE
        .try_with(|d| d.saturating_duration_since(Instant::now()))
        .ok()
}

/// The deadline of the request being handled, when it has one.
#[must_use]
pub fn current_deadline() -> Option<Instant> {
    DEADLINE.try_with(|d| *d).ok()
}

/// Parse a gRPC `grpc-timeout` value: a positive integer and a unit character.
///
/// `H`ours, `M`inutes, `S`econds, `m`illiseconds, `u`microseconds,
/// `n`anoseconds - the units the gRPC wire specification defines.
///
/// # Arguments
///
/// * `value` - A `grpc-timeout` header value: digits then one unit character.
///   Anything else is `None`, which means no deadline rather than a wrong one.
#[must_use]
pub fn parse_grpc_timeout(value: &str) -> Option<Duration> {
    let value = value.trim();
    let (digits, unit) = value.split_at(value.len().checked_sub(1)?);
    let n: u64 = digits.parse().ok()?;
    match unit {
        "H" => Some(Duration::from_secs(n.checked_mul(3600)?)),
        "M" => Some(Duration::from_secs(n.checked_mul(60)?)),
        "S" => Some(Duration::from_secs(n)),
        "m" => Some(Duration::from_millis(n)),
        "u" => Some(Duration::from_micros(n)),
        "n" => Some(Duration::from_nanos(n)),
        _ => None,
    }
}

/// Render a `Duration` in the gRPC timeout format, always in milliseconds.
///
/// # Arguments
///
/// * `d` - The remaining budget. Always rendered in milliseconds, so successive
///   hops cannot compound a rounding error.
#[must_use]
pub fn format_grpc_timeout(d: Duration) -> HeaderValue {
    let ms = u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
    HeaderValue::from_str(&format!("{ms}m")).unwrap_or_else(|_| HeaderValue::from_static("0m"))
}

/// Enforces a deadline and publishes it as the [`DEADLINE`] task-local.
///
/// The effective deadline is the sooner of the caller's `grpc-timeout` and the
/// configured default, so a caller may ask for less time but never for more.
#[derive(Debug, Clone, Copy)]
pub struct DeadlineLayer {
    /// The deadline applied when the caller asks for none.
    default: Option<Duration>,
    /// Whether to answer a timeout as `grpc-status: 4` rather than HTTP 504.
    grpc_status: bool,
}

impl DeadlineLayer {
    /// A layer whose requests time out after `default` unless the caller asked
    /// for less. `None` means only a caller-supplied deadline applies.
    ///
    /// # Arguments
    ///
    /// * `default` - The ceiling for a request that asked for nothing. `None`
    ///   means only a caller-supplied deadline applies, and a caller asking for
    ///   more than this still gets this.
    #[must_use]
    pub fn new(default: Option<Duration>) -> Self {
        Self {
            default,
            grpc_status: false,
        }
    }

    /// Answer an expired deadline the way gRPC expects - HTTP 200 with
    /// `grpc-status: 4` - rather than with an HTTP 504.
    #[must_use]
    pub fn grpc(mut self) -> Self {
        self.grpc_status = true;
        self
    }
}

impl<S> Layer<S> for DeadlineLayer {
    type Service = DeadlineService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        DeadlineService {
            inner,
            default: self.default,
            grpc_status: self.grpc_status,
        }
    }
}

/// The service [`DeadlineLayer`] produces.
#[derive(Debug, Clone, Copy)]
pub struct DeadlineService<S> {
    /// The wrapped service.
    inner: S,
    /// The deadline applied when the caller asks for none.
    default: Option<Duration>,
    /// Whether to answer a timeout as `grpc-status: 4` rather than HTTP 504.
    grpc_status: bool,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for DeadlineService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Default,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = DeadlineFuture<S::Future, ResBody>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let from_caller = req
            .headers()
            .get(GRPC_TIMEOUT)
            .and_then(|v| v.to_str().ok())
            .and_then(parse_grpc_timeout);

        let budget = match (from_caller, self.default) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (Some(d), None) | (None, Some(d)) => Some(d),
            (None, None) => None,
        };

        // Always scope the task-local so there is one future type. Without a
        // deadline the value is far enough out that `time_remaining` is
        // effectively unbounded, and the sleep never fires.
        let deadline = budget.map_or_else(
            || Instant::now() + Duration::from_secs(86_400 * 365),
            |d| Instant::now() + d,
        );

        DeadlineFuture {
            inner: DEADLINE.scope(deadline, self.inner.call(req)),
            sleep: budget.map(|d| Box::pin(tokio::time::sleep(d))),
            grpc_status: self.grpc_status,
            _body: std::marker::PhantomData,
        }
    }
}

pin_project! {
    /// The future of [`DeadlineService`].
    pub struct DeadlineFuture<F, B> {
        #[pin]
        inner: TaskLocalFuture<Instant, F>,
        sleep: Option<std::pin::Pin<Box<Sleep>>>,
        grpc_status: bool,
        _body: std::marker::PhantomData<fn() -> B>,
    }
}

impl<F, ResBody, E> Future for DeadlineFuture<F, ResBody>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    ResBody: Default,
{
    type Output = Result<Response<ResBody>, E>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        if let Poll::Ready(res) = this.inner.poll(cx) {
            return Poll::Ready(res);
        }
        if let Some(sleep) = this.sleep.as_mut()
            && sleep.as_mut().poll(cx).is_ready()
        {
            return Poll::Ready(Ok(expired(*this.grpc_status)));
        }
        Poll::Pending
    }
}

/// The response for a request whose deadline passed before the inner service
/// answered.
///
/// # Arguments
///
/// * `grpc_status` - Whether to answer in gRPC terms. `true` gives
///   `DEADLINE_EXCEEDED`, `false` gives HTTP 504.
fn expired<B: Default>(grpc_status: bool) -> Response<B> {
    let mut res = Response::new(B::default());
    if grpc_status {
        // gRPC reports errors in trailers-like headers on a 200, not in the
        // HTTP status: a 504 here is a transport failure the client retries.
        res.headers_mut()
            .insert("grpc-status", HeaderValue::from_static("4"));
        res.headers_mut().insert(
            "grpc-message",
            HeaderValue::from_static("deadline exceeded"),
        );
    } else {
        *res.status_mut() = StatusCode::GATEWAY_TIMEOUT;
    }
    res
}
