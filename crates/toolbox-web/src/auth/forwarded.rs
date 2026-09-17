//! Populating the caller's `Principal` from an authenticating proxy's headers.
//!
//! `toolbox-auth` has no `http` dependency by design - a backend validates a
//! token without compiling axum - so it owns only the transport-neutral half:
//! [`ForwardedIdentity`] (a plain struct) and [`ForwardedIdentityProvider`]
//! (the trust check). This module is the axum glue that reads the real request
//! headers and the `ConnectInfo` peer and fills that struct.

use std::net::SocketAddr;

use axum::{
    extract::{ConnectInfo, Request, State},
    middleware::Next,
    response::Response,
};
use http::{Extensions, HeaderMap};
use secrecy::SecretString;
use toolbox_auth::{
    Credential, ForwardedHeaders, ForwardedIdentity, ForwardedIdentityProvider, Principal,
};

use super::AuthState;

/// What [`forwarded_auth_layer`] needs beyond the request itself.
///
/// The header names come from the [`ForwardedIdentityProvider`] that will do
/// the trust check, so the layer reads exactly the headers the provider
/// consults - they cannot drift apart.
#[derive(Debug, Clone)]
pub struct ForwardedConfig {
    /// The header names, copied from the provider.
    headers: ForwardedHeaders,
    /// The header carrying the proxy's shared secret, when the registry's
    /// [`ForwardedIdentityProvider`] trusts a secret rather than a peer list.
    /// Its value is read into [`ForwardedIdentity::secret`]; unset means the
    /// header is not read.
    pub secret_header: Option<String>,
}

impl ForwardedConfig {
    /// A config reading oauth2-proxy's `X-Forwarded-*` header names.
    ///
    /// Use [`for_provider`](Self::for_provider) instead when the registry holds
    /// a [`ForwardedIdentityProvider`] configured for different names.
    #[must_use]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            headers: ForwardedHeaders::default(),
            secret_header: None,
        }
    }

    /// Read the header names from the provider that will do the trust check.
    ///
    /// # Arguments
    ///
    /// * `provider` - The registry's forwarded-identity provider. Its header
    ///   names are copied here so the layer and the provider never disagree
    ///   about which headers carry the identity.
    #[must_use]
    pub fn for_provider(provider: &ForwardedIdentityProvider) -> Self {
        Self {
            headers: provider.headers().clone(),
            secret_header: None,
        }
    }

    /// The header carrying the proxy's shared secret.
    ///
    /// # Arguments
    ///
    /// * `header` - The header name whose value is read into
    ///   [`ForwardedIdentity::secret`], for a secret-anchored provider.
    #[must_use]
    pub fn secret_header(mut self, header: impl Into<String>) -> Self {
        self.secret_header = Some(header.into());
        self
    }
}

/// Populate the caller's `Principal` from an authenticating proxy's headers.
///
/// The registry's [`ForwardedIdentityProvider`] does the trust check - a
/// trusted-peer list or a shared secret; this layer only reads the headers and
/// the peer and hands them over. Mount it *after* [`session_layer`] so a real
/// bearer token wins over a forwarded header.
///
/// [`session_layer`]: super::session_layer
///
/// # Arguments
///
/// * `state` - The application state and the header configuration, passed as a
///   pair to `axum::middleware::from_fn_with_state`.
/// * `request` - The incoming request. Its forwarded headers and peer are read.
/// * `next` - The rest of the stack.
pub async fn forwarded_auth_layer<S: AuthState>(
    State((state, config)): State<(S, ForwardedConfig)>,
    mut request: Request,
    next: Next,
) -> Response {
    // The proxy header is the fallback, not an override: a bearer token that
    // already resolved wins.
    if request.extensions().get::<Principal>().is_none() {
        let identity = forwarded_identity(request.headers(), request.extensions(), &config);
        if identity.user.is_some()
            && let Ok(principal) = state
                .providers()
                .authenticate(&Credential::Custom(Box::new(identity)))
                .await
        {
            request.extensions_mut().insert(principal);
        }
    }
    next.run(request).await
}

/// Read a [`ForwardedIdentity`] out of a request.
///
/// # Arguments
///
/// * `headers` - The request headers.
/// * `extensions` - The request extensions, where the connect info lives.
/// * `config` - Which headers to read.
fn forwarded_identity(
    headers: &HeaderMap,
    extensions: &Extensions,
    config: &ForwardedConfig,
) -> ForwardedIdentity {
    let read = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .map(str::to_owned)
            .filter(|s| !s.is_empty())
    };
    ForwardedIdentity {
        user: read(&config.headers.user),
        groups: read(&config.headers.groups),
        email: read(&config.headers.email),
        // Trust is anchored to the proxy that opened this TCP connection, not
        // to an end-user address that proxy reported in a forwarded header.
        peer: extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|ConnectInfo(peer)| peer.ip()),
        secret: config
            .secret_header
            .as_deref()
            .and_then(read)
            .map(SecretString::from),
    }
}
