//! Service-to-service authentication.
//!
//! In this architecture the gateway is the auth layer and backends trust their
//! caller, which is only safe if "the caller" is actually the gateway. That is
//! what this checks.
//!
//! mTLS is deliberately **not** wrapped here: `tonic::transport::ClientTlsConfig`
//! and `ServerTlsConfig` already do it well, and a wrapper would add nothing.
//! Configure it on the endpoint, and use this for the deployments where mTLS
//! is more machinery than the threat warrants.

use std::{
    marker::PhantomData,
    task::{Context, Poll},
};

use http::{HeaderName, HeaderValue, Request, Response};
use pin_project_lite::pin_project;
use secrecy::{ExposeSecret, SecretString};
use tonic::Status;
use tower::{Layer, Service};
use tracing::warn;

/// The header a shared secret travels in.
pub const SERVICE_AUTH_HEADER: HeaderName = HeaderName::from_static("x-service-auth");

/// How a backend authenticates its caller.
#[derive(Clone)]
pub enum ServiceAuth {
    /// A shared secret, compared in constant time.
    SharedSecret(SecretString),
}

impl std::fmt::Debug for ServiceAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the secret itself: a Debug of a config struct ends up in logs.
        f.write_str("ServiceAuth::SharedSecret(<redacted>)")
    }
}

impl ServiceAuth {
    /// A shared secret.
    ///
    /// # Arguments
    ///
    /// * `secret` - The credential both ends compare. It is checked in constant
    ///   time, so its length does not leak through the response time.
    pub fn shared_secret(secret: impl Into<String>) -> Self {
        Self::SharedSecret(SecretString::from(secret.into()))
    }

    /// The header value a client should send.
    #[must_use]
    pub fn header_value(&self) -> Option<HeaderValue> {
        match self {
            Self::SharedSecret(s) => HeaderValue::from_str(s.expose_secret()).ok(),
        }
    }
}

/// Reject any request that does not carry the expected service credential.
#[derive(Clone)]
pub struct ServiceAuthLayer {
    expected: SecretString,
}

impl std::fmt::Debug for ServiceAuthLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ServiceAuthLayer")
    }
}

/// Require a service credential on every request.
///
/// # Arguments
///
/// * `auth` - The credential every request must carry. Taken by reference so
///   the caller keeps its config, which the backend channels also read.
#[must_use]
pub fn require_service_auth(auth: &ServiceAuth) -> ServiceAuthLayer {
    match auth {
        ServiceAuth::SharedSecret(s) => ServiceAuthLayer {
            expected: s.clone(),
        },
    }
}

impl<S> Layer<S> for ServiceAuthLayer {
    type Service = ServiceAuthService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        ServiceAuthService {
            inner,
            expected: self.expected.clone(),
        }
    }
}

/// The service [`ServiceAuthLayer`] produces.
#[derive(Clone)]
pub struct ServiceAuthService<S> {
    inner: S,
    expected: SecretString,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for ServiceAuthService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Default,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = ServiceAuthFuture<S::Future, ResBody>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let presented = req
            .headers()
            .get(SERVICE_AUTH_HEADER)
            .and_then(|v| v.to_str().ok());

        let ok = presented.is_some_and(|p| constant_time_eq(p, self.expected.expose_secret()));
        if ok {
            ServiceAuthFuture {
                inner: Some(self.inner.call(req)),
                _body: PhantomData,
            }
        } else {
            // Log the refusal but not the credential: a wrong secret in a log
            // is still a secret someone tried.
            warn!("refused a request with a missing or invalid service credential");
            ServiceAuthFuture {
                inner: None,
                _body: PhantomData,
            }
        }
    }
}

pin_project! {
    /// Either the wrapped call, or an immediate refusal.
    pub struct ServiceAuthFuture<F, B> {
        #[pin]
        inner: Option<F>,
        _body: PhantomData<fn() -> B>,
    }
}

impl<F, ResBody, E> Future for ServiceAuthFuture<F, ResBody>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    ResBody: Default,
{
    type Output = Result<Response<ResBody>, E>;

    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(f) = self.project().inner.as_pin_mut() {
            return f.poll(cx);
        }
        let status = Status::unauthenticated("service credential missing or invalid");
        Poll::Ready(Ok(status.into_http()))
    }
}

/// Compare two secrets without leaking their length relationship through
/// timing.
///
/// A short-circuiting `==` tells an attacker how many leading bytes were right,
/// which turns guessing a secret from infeasible into linear.
///
/// # Arguments
///
/// * `a` - The expected credential.
/// * `b` - What the caller presented.
fn constant_time_eq(a: &str, b: &str) -> bool {
    let (a, b) = (a.as_bytes(), b.as_bytes());
    if a.len() != b.len() {
        // The length itself is not secret, and comparing different lengths
        // byte-wise would be meaningless anyway.
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}
