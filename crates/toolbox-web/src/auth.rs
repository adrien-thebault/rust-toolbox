//! The login routes and the session middleware.
//!
//! Login, refresh, logout and "who am I" are the same four endpoints in every
//! project, and two of them have a security-relevant detail that is easy to get
//! wrong.
//!
//! Two things here that are otherwise re-derived per project:
//!
//! - **The credential rate limit is attached here**, to `/auth/login` and
//!   `/auth/refresh` and to nothing else. Wiring it by hand means reasoning
//!   about axum's "a layer only wraps routes already added" ordering rule
//!   every time, and getting it wrong throttles `/auth/me` on every page load.
//! - **Refresh tokens are stateless JWTs.** A short access token with no
//!   refresh logs the user out constantly; a long one is a revocation window
//!   nobody wants. The refresh token carries the principal and, optionally, a
//!   fingerprint of the stored credential so "change your password" revokes it
//!   ([`AuthState::refresh_epoch`]).

use std::sync::Arc;

use axum::{
    Json, Router,
    extract::State,
    response::IntoResponse,
    routing::{get, post},
};
use governor::{clock::QuantaInstant, middleware::NoOpMiddleware};
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use toolbox_auth::{
    AuthError, Credential, ForwardedHeaders, ForwardedIdentity, JwtIdentityProvider, Principal,
    ProviderRegistry, RefreshInfo,
};
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};
use tracing::debug;

use crate::{
    client_ip::{TrustedHops, client_ip_of},
    error::ApiError,
    rate_limit::{ForwardedForKeyExtractor, error_response_handler},
};

/// What the auth routes need from the application's state.
pub trait AuthState: Clone + Send + Sync + 'static {
    /// Everything a caller may present, including the bearer verifier.
    fn providers(&self) -> &ProviderRegistry;

    /// The codec that mints this gateway's sessions.
    fn session_issuer(&self) -> &JwtIdentityProvider;

    /// The credential fingerprint to bind a **new** refresh token to, at login.
    ///
    /// Return `Some(toolbox_auth::auth_epoch(secret, &stored_hash))` to make
    /// "change your password" invalidate every refresh token for that user. The
    /// default, `None`, issues refresh tokens with no credential binding.
    ///
    /// # Arguments
    ///
    /// * `_principal` - Who the refresh token will be for, fresh from login.
    fn refresh_epoch(
        &self,
        _principal: &Principal,
    ) -> impl std::future::Future<Output = Option<String>> + Send {
        std::future::ready(None)
    }

    /// Re-resolve a principal when a refresh token is redeemed.
    ///
    /// Given what the token carried ([`RefreshInfo`]), return the principal
    /// **as it is now** - re-read your user store so a demotion or a disabled
    /// account takes effect on the next refresh, not after the full refresh
    /// TTL. Return `Err(AuthError::Unauthenticated)` to reject: the account is
    /// gone, or `info.epoch` no longer matches the stored credential. The
    /// default trusts the token as-is.
    fn resolve_refresh(
        &self,
        info: RefreshInfo,
    ) -> impl std::future::Future<Output = Result<Principal, AuthError>> + Send {
        std::future::ready(Ok(info.stale))
    }
}

/// A login request.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    /// The username.
    pub username: String,
    /// The password.
    pub password: String,
}

/// What a successful login returns.
#[derive(Debug, Serialize)]
pub struct SessionResponse {
    /// The session token, for the `Authorization: Bearer` header.
    pub access_token: String,
    /// Always `Bearer`.
    pub token_type: &'static str,
    /// How many seconds the access token is good for.
    pub expires_in: u64,
    /// The refresh token, to trade for a new access token later.
    pub refresh_token: String,
}

/// A refresh request.
#[derive(Debug, Deserialize)]
pub struct RefreshRequest {
    /// The refresh token previously issued.
    pub refresh_token: String,
}

/// How the credential routes are throttled.
///
/// Unauthenticated endpoints that check a secret are the ones worth limiting:
/// without this, an attacker gets as many password guesses per second as the
/// argon2 cost allows.
#[derive(Debug, Clone, Copy)]
pub struct LoginLimit {
    /// How many attempts one caller may make back to back.
    pub burst: u32,
    /// How long before one of those attempts is replenished.
    pub replenish_every: std::time::Duration,
    /// How many proxies append to `X-Forwarded-For`.
    ///
    /// It must match what the rest of the process uses. Set too low behind a
    /// proxy, the limiter keys on the proxy's address and the first attacker
    /// locks out every other caller.
    pub hops: TrustedHops,
}

impl Default for LoginLimit {
    /// Five attempts, then one every five seconds.
    ///
    /// Enough that a person mistyping a password three times notices nothing,
    /// slow enough that credential stuffing from one address is pointless.
    fn default() -> Self {
        Self {
            burst: 5,
            replenish_every: std::time::Duration::from_secs(5),
            hops: TrustedHops::default(),
        }
    }
}

/// The auth routes.
///
/// Mount under whatever prefix you like; the paths here are relative.
///
/// # Arguments
///
/// * `limit` - How `/auth/login` and `/auth/refresh` are throttled. Taken
///   rather than defaulted because `hops` has no safe default: guessing it
///   wrong is what turns a per-caller limit into a global one.
pub fn auth_router<S: AuthState>(limit: &LoginLimit) -> Router<S> {
    Router::new()
        .route("/auth/login", post(login::<S>))
        .route("/auth/refresh", post(refresh::<S>))
        // Applied to the two routes above and no others, because a layer wraps
        // what was already added. `/auth/me` is called on every page load.
        .layer(login_limiter(limit))
        .route("/auth/logout", post(logout))
        .route("/auth/me", get(me))
}

/// The throttle applied to the credential routes.
///
/// Private, so the `expect` below is not a documented panic: `burst` and the
/// period are both clamped above zero here, which is the only way `finish`
/// returns `None`.
///
/// # Arguments
///
/// * `limit` - What to build the limiter from.
fn login_limiter(
    limit: &LoginLimit,
) -> GovernorLayer<ForwardedForKeyExtractor, NoOpMiddleware<QuantaInstant>, axum::body::Body> {
    let config = GovernorConfigBuilder::default()
        .key_extractor(ForwardedForKeyExtractor::new(limit.hops))
        .per_millisecond(
            u64::try_from(limit.replenish_every.as_millis())
                .unwrap_or(u64::MAX)
                .max(1),
        )
        .burst_size(limit.burst.max(1))
        .finish()
        .expect("a non-zero burst and period");

    GovernorLayer::new(Arc::new(config)).error_handler(error_response_handler)
}

/// `POST`: exchange a credential for a session.
///
/// # Arguments
///
/// * `state` - The application state, which is where the registry and the
///   session issuer are reached.
/// * `body` - The credential presented. Every registered provider is tried in
///   order until one claims it.
async fn login<S: AuthState>(
    State(state): State<S>,
    Json(body): Json<LoginRequest>,
) -> Result<Json<SessionResponse>, ApiError> {
    let credential = Credential::Password {
        username: body.username,
        password: SecretString::from(body.password),
    };

    let principal = state.providers().authenticate(&credential).await?;
    issue(&state, &principal).await
}

/// `POST`: redeem a refresh token and mint a new session.
///
/// # Arguments
///
/// * `state` - The application state.
/// * `body` - The refresh token being redeemed.
async fn refresh<S: AuthState>(
    State(state): State<S>,
    Json(body): Json<RefreshRequest>,
) -> Result<Json<SessionResponse>, ApiError> {
    let issuer = state.session_issuer();
    let refreshed = issuer
        .refresh(&body.refresh_token, |info| state.resolve_refresh(info))
        .await?;

    Ok(Json(SessionResponse {
        access_token: refreshed.access_token,
        token_type: "Bearer",
        expires_in: issuer.token_ttl().as_secs(),
        refresh_token: refreshed.refresh_token,
    }))
}

/// `POST`: end the caller's session.
///
/// With stateless sessions there is nothing server-side to revoke: the client
/// discards its tokens and the access token expires on its own. "Log out
/// everywhere" is a password change. The endpoint stays so a client has one
/// call to make.
#[allow(clippy::unused_async)]
async fn logout() -> http::StatusCode {
    http::StatusCode::NO_CONTENT
}

/// `GET`: the caller's principal, so a frontend can render its own interface
/// without re-deriving roles from the token.
///
/// # Arguments
///
/// * `principal` - What the session layer put in the extensions. Absent means
///   unauthenticated, which is a 401 here.
#[allow(clippy::unused_async)]
async fn me(principal: Option<axum::Extension<Principal>>) -> Result<Json<Principal>, ApiError> {
    principal
        .map(|axum::Extension(p)| Json(p))
        .ok_or_else(|| AuthError::Unauthenticated.into())
}

/// Build the session response, so login and refresh cannot drift apart.
///
/// # Arguments
///
/// * `state` - The application state, for the issuer and the epoch hook.
/// * `principal` - Who the session is for.
async fn issue<S: AuthState>(
    state: &S,
    principal: &Principal,
) -> Result<Json<SessionResponse>, ApiError> {
    let issuer = state.session_issuer();
    let epoch = state.refresh_epoch(principal).await;

    Ok(Json(SessionResponse {
        access_token: issuer.issue(principal)?,
        token_type: "Bearer",
        expires_in: issuer.token_ttl().as_secs(),
        refresh_token: issuer.issue_refresh(principal, epoch.as_deref())?,
    }))
}

/// Put the caller's `Principal` in the request extensions, when they have one.
///
/// Deliberately does **not** reject an unauthenticated request: that is
/// `Authenticated<R>`'s job, in the handler signature where it is visible.
/// A layer that rejects makes every public route need an exception.
///
/// # Arguments
///
/// * `state` - The application state, read for the provider registry.
/// * `request` - The incoming request. Its `Authorization` header is read, and
///   the principal is inserted into its extensions.
/// * `next` - The rest of the stack, called whether or not a principal was
///   found.
pub async fn session_layer<S: AuthState>(
    State(state): State<S>,
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
    if let Some(token) = bearer(&request) {
        match state
            .providers()
            .authenticate(&Credential::Bearer(SecretString::from(token)))
            .await
        {
            Ok(principal) => {
                request.extensions_mut().insert(principal);
            }
            // An expired token has to reach the client as a 401 so it knows to
            // refresh; continuing anonymous would turn it into a 403 from
            // whatever came next.
            Err(AuthError::Expired) => return ApiError::from(AuthError::Expired).into_response(),
            Err(_) => {
                debug!("a request carried a session that did not verify");
            }
        }
    }
    next.run(request).await
}

/// The bearer token from an `Authorization` header, if there is one.
///
/// # Arguments
///
/// * `request` - The request to read. A missing or malformed header is `None`,
///   not an error: rejecting is the extractor's job.
fn bearer(request: &axum::extract::Request) -> Option<String> {
    request
        .headers()
        .get(http::header::AUTHORIZATION)?
        .to_str()
        .ok()?
        .strip_prefix("Bearer ")
        .map(str::to_owned)
}

/// What [`forwarded_auth_layer`] needs: which headers a trusting proxy sets,
/// and how far back the peer address is.
#[derive(Debug, Clone, Default)]
pub struct ForwardedConfig {
    /// The header names this proxy uses. Defaults to oauth2-proxy's
    /// `X-Forwarded-*`; `ForwardedHeaders::authelia()` and friends cover the
    /// rest.
    pub headers: ForwardedHeaders,
    /// How many proxy hops to trust when resolving the peer address, matching
    /// what the rest of the process uses.
    pub hops: TrustedHops,
    /// The header carrying the proxy's shared secret, when the registry's
    /// [`toolbox_auth::ForwardedIdentityProvider`] trusts a secret rather than
    /// a peer list. Its value is read into
    /// [`toolbox_auth::ForwardedIdentity::secret`]; unset means the header is
    /// not read.
    pub secret_header: Option<String>,
}

/// Populate the caller's `Principal` from an authenticating proxy's headers.
///
/// The registry's [`toolbox_auth::ForwardedIdentityProvider`] does the peer
/// check: this layer only reads the headers and the peer and hands them over.
/// Mount it *after* [`session_layer`] so a real bearer token wins over a
/// forwarded header.
///
/// # Arguments
///
/// * `state` - The application state and the header configuration, passed as a
///   pair to `axum::middleware::from_fn_with_state`.
/// * `request` - The incoming request. Its forwarded headers and peer are read.
/// * `next` - The rest of the stack.
pub async fn forwarded_auth_layer<S: AuthState>(
    State((state, config)): State<(S, ForwardedConfig)>,
    mut request: axum::extract::Request,
    next: axum::middleware::Next,
) -> axum::response::Response {
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
/// * `config` - Which headers to read and how many hops to trust.
fn forwarded_identity(
    headers: &http::HeaderMap,
    extensions: &http::Extensions,
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
        peer: client_ip_of(headers, extensions, config.hops),
        secret: config
            .secret_header
            .as_deref()
            .and_then(read)
            .map(SecretString::from),
    }
}
