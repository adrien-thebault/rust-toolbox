//! Resolving the caller's identity from the request, and reading it in a
//! handler.
//!
//! [`identity_layer`] runs whatever credential its [`IdentityLayer::extracting`]
//! sources find through a [`ProviderRegistry`] and stashes the resolved
//! [`Principal`] for a handler to read with [`optional`] or [`require`]. Nothing
//! is extracted by default - compose the two provided sources, and any of your
//! own:
//!
//! ```ignore
//! identity_layer(registry)
//!     .extracting(forwarded_principal) // the gateway's `x-fwd-principal`
//!     .extracting(bearer)              // a direct `authorization: Bearer`
//! ```
//!
//! Put [`super::shared_secret::shared_secret_layer`] in front: this trusts
//! whatever reached it.

use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use http::{HeaderMap, Request, Response, header::AUTHORIZATION};
use secrecy::SecretString;
use tonic::Status;
use toolbox_auth::{Credential, ForwardedPrincipal, Principal, ProviderRegistry};
use tower::{Layer, Service};

use crate::X_FWD_PRINCIPAL;

/// Pulls a [`Credential`] out of a request's headers, or `None` if this one is
/// not there.
type Extractor = dyn Fn(&HeaderMap) -> Option<Credential> + Send + Sync;

/// The caller, if an earlier [`identity_layer`] resolved one.
///
/// # Arguments
///
/// * `request` - The request a handler received.
#[must_use]
pub fn optional<T>(request: &tonic::Request<T>) -> Option<&Principal> {
    request.extensions().get::<Principal>()
}

/// The caller, or [`Status::unauthenticated`] when the call carried no user.
///
/// The first line of a handler that needs an end user rather than just a
/// trusted caller.
///
/// # Arguments
///
/// * `request` - The request a handler received.
///
/// # Errors
/// [`Status::unauthenticated`] when no identity was resolved.
pub fn require<T>(request: &tonic::Request<T>) -> Result<&Principal, Status> {
    optional(request)
        .ok_or_else(|| Status::unauthenticated("this call requires an authenticated user"))
}

/// A layer that resolves the caller's identity through `registry` and stashes it
/// in the request extensions, for handlers to read with [`optional`] or
/// [`require`].
///
/// It extracts nothing on its own - add [`IdentityLayer::extracting`] sources
/// ([`forwarded_principal`], [`bearer`], your own). With none, every request
/// proceeds with no principal. A missing or unresolvable credential is not an
/// error either.
///
/// # Arguments
///
/// * `registry` - The providers a found credential is resolved through.
#[must_use]
pub fn identity_layer(registry: Arc<ProviderRegistry>) -> IdentityLayer {
    IdentityLayer {
        registry,
        extractors: Vec::new(),
    }
}

/// The gateway's forwarded principal, from `x-fwd-principal`.
///
/// Pass to [`IdentityLayer::extracting`] for a service behind the toolbox
/// gateway; pair it with a `ForwardedPrincipalProvider` in the registry.
///
/// # Arguments
///
/// * `headers` - The request headers.
#[must_use]
pub fn forwarded_principal(headers: &HeaderMap) -> Option<Credential> {
    let encoded = headers.get(X_FWD_PRINCIPAL)?.to_str().ok()?;
    ForwardedPrincipal::decode(encoded)
        .ok()
        .map(|f| Credential::Custom(Box::new(f)))
}

/// A bearer token from the `Authorization` header.
///
/// Pass to [`IdentityLayer::extracting`] for a service also reachable directly;
/// pair it with a `JwtIdentityProvider` (or JWKS) in the registry.
///
/// # Arguments
///
/// * `headers` - The request headers.
#[must_use]
pub fn bearer(headers: &HeaderMap) -> Option<Credential> {
    let token = headers
        .get(AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")?;
    Some(Credential::Bearer(SecretString::from(token)))
}

/// The first credential any source finds, in the order they were added.
fn credential(extractors: &[Arc<Extractor>], headers: &HeaderMap) -> Option<Credential> {
    extractors.iter().find_map(|extract| extract(headers))
}

/// The layer [`identity_layer`] produces.
#[derive(Clone)]
pub struct IdentityLayer {
    /// The providers a credential is resolved through.
    registry: Arc<ProviderRegistry>,
    /// The credential sources, tried in order.
    extractors: Vec<Arc<Extractor>>,
}

impl IdentityLayer {
    /// Add a credential source, tried after the ones already added.
    ///
    /// [`forwarded_principal`] and [`bearer`] are the provided two; a deployment
    /// whose registry has a provider on some other credential adds its own.
    ///
    /// # Arguments
    ///
    /// * `extract` - Reads the request headers and returns a [`Credential`], or
    ///   `None` when this source is not present.
    #[must_use]
    pub fn extracting(
        mut self,
        extract: impl Fn(&HeaderMap) -> Option<Credential> + Send + Sync + 'static,
    ) -> Self {
        self.extractors.push(Arc::new(extract));
        self
    }
}

impl std::fmt::Debug for IdentityLayer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityLayer")
            .field("extractors", &self.extractors.len())
            .finish_non_exhaustive()
    }
}

impl<S> Layer<S> for IdentityLayer {
    type Service = IdentityService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        IdentityService {
            inner,
            registry: Arc::clone(&self.registry),
            extractors: self.extractors.clone(),
        }
    }
}

/// The service [`IdentityLayer`] produces.
#[derive(Clone)]
pub struct IdentityService<S> {
    /// The wrapped service.
    inner: S,
    /// The providers a credential is resolved through.
    registry: Arc<ProviderRegistry>,
    /// The credential sources, tried in order.
    extractors: Vec<Arc<Extractor>>,
}

impl<S: tonic::server::NamedService> tonic::server::NamedService for IdentityService<S> {
    const NAME: &'static str = S::NAME;
}

impl<S, ReqBody, ResBody> Service<Request<ReqBody>> for IdentityService<S>
where
    S: Service<Request<ReqBody>, Response = Response<ResBody>> + Clone + Send + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = Pin<Box<dyn Future<Output = Result<S::Response, S::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, mut req: Request<ReqBody>) -> Self::Future {
        // Clone-and-swap so the async block owns a ready inner service.
        let clone = self.inner.clone();
        let mut inner = std::mem::replace(&mut self.inner, clone);
        let registry = Arc::clone(&self.registry);
        let extractors = self.extractors.clone();

        Box::pin(async move {
            if let Some(cred) = credential(&extractors, req.headers())
                && let Ok(principal) = registry.authenticate(&cred).await
            {
                req.extensions_mut().insert(principal);
            }
            inner.call(req).await
        })
    }
}
