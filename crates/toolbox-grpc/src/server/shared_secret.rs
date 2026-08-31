//! The shared-secret gate: is this an allowed caller?
//!
//! Caller authorization, not identity - it answers only whether the request
//! holds the deployment's service secret. It belongs at the door of every
//! service that trusts its gateway; [`super::identity`] answers who the end
//! user is, on top.

use std::{
    future::Future,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use http::{Request, Response};
use pin_project_lite::pin_project;
use secrecy::{ExposeSecret as _, SecretString};
use tonic::Status;
use toolbox_auth::constant_time_eq;
use tower::{Layer, Service};
use tracing::warn;

use crate::X_SHARED_SECRET;

/// Reject any request that does not carry the expected shared secret.
///
/// # Arguments
///
/// * `secret` - The value a caller must present in `x-shared-secret`, compared
///   in constant time.
#[must_use]
pub fn shared_secret_layer(secret: impl Into<String>) -> SharedSecretLayer {
    SharedSecretLayer {
        expected: SecretString::from(secret.into()),
    }
}

/// The layer [`shared_secret_layer`] produces.
#[derive(Clone)]
pub struct SharedSecretLayer {
    /// The secret every request must present.
    expected: SecretString,
}

impl std::fmt::Debug for SharedSecretLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SharedSecretLayer")
    }
}

impl<S> Layer<S> for SharedSecretLayer {
    type Service = SharedSecretService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        SharedSecretService {
            inner,
            expected: self.expected.clone(),
        }
    }
}

/// The service [`SharedSecretLayer`] produces.
#[derive(Clone)]
pub struct SharedSecretService<S> {
    /// The wrapped service.
    inner: S,
    /// The secret every request must present.
    expected: SecretString,
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for SharedSecretService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>>,
    ResBody: Default,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = SharedSecretFuture<S::Future, ResBody>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        let presented = req
            .headers()
            .get(X_SHARED_SECRET)
            .and_then(|v| v.to_str().ok());

        if presented.is_some_and(|p| {
            constant_time_eq(p.as_bytes(), self.expected.expose_secret().as_bytes())
        }) {
            SharedSecretFuture {
                inner: Some(self.inner.call(req)),
                _body: PhantomData,
            }
        } else {
            warn!("refused a request with a missing or invalid shared service secret");
            SharedSecretFuture {
                inner: None,
                _body: PhantomData,
            }
        }
    }
}

impl<S: tonic::server::NamedService> tonic::server::NamedService for SharedSecretService<S> {
    const NAME: &'static str = S::NAME;
}

pin_project! {
    /// Either the wrapped call, or an immediate refusal.
    pub struct SharedSecretFuture<F, B> {
        #[pin]
        inner: Option<F>,
        _body: PhantomData<fn() -> B>,
    }
}

impl<F, ResBody, E> Future for SharedSecretFuture<F, ResBody>
where
    F: Future<Output = Result<Response<ResBody>, E>>,
    ResBody: Default,
{
    type Output = Result<Response<ResBody>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(f) = self.project().inner.as_pin_mut() {
            return f.poll(cx);
        }
        Poll::Ready(Ok(Status::unauthenticated(
            "shared service secret missing or invalid",
        )
        .into_http()))
    }
}
