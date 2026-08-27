//! The login routes and the session middleware.
//!
//! Login, refresh, logout, "who am I" and "what can I log in with" are the same
//! five endpoints in every project, and three of them have a security-relevant
//! detail that is easy to get wrong.
//!
//! Three things here that are otherwise re-derived per project:
//!
//! - **The credential rate limit is attached here**, to `/auth/login` and
//!   `/auth/refresh` and to nothing else. Wiring it by hand means reasoning
//!   about axum's "a layer only wraps routes already added" ordering rule
//!   every time, and getting it wrong throttles `/auth/me` on every page load.
//! - **Refresh tokens exist.** A twelve-hour access token with no refresh
//!   means the admin is silently logged out mid-session; a short one without
//!   refresh means they are logged out constantly.
//! - **`GET /auth/providers`** lets a login page render "Sign in with
//!   Keycloak" without a frontend change. Small endpoint, large decoupling.

use std::{sync::Arc, time::Duration};

use axum::{
    Json, Router,
    extract::State,
    response::IntoResponse,
    routing::{get, post},
};
use governor::{clock::QuantaInstant, middleware::NoOpMiddleware};
use http::StatusCode;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use toolbox_auth::{
    AuthError, Credential, Principal, ProviderInfo, ProviderRegistry, RefreshTokens, SessionCodec,
};
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};
use tracing::debug;

use crate::{
    client_ip::TrustedHops,
    error::ApiError,
    rate_limit::{ForwardedForKeyExtractor, error_response_handler},
};

/// What the auth routes need from the application's state.
pub trait AuthState: Clone + Send + Sync + 'static {
    /// Everything a caller may log in with.
    fn providers(&self) -> &ProviderRegistry;
    /// How sessions are issued and verified.
    fn sessions(&self) -> &SessionCodec;
    /// Refresh tokens, when the deployment issues them.
    fn refresh_tokens(&self) -> Option<&Arc<RefreshTokens>> {
        None
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
    /// The refresh token, when the deployment issues them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
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
    pub replenish_every: Duration,
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
            replenish_every: Duration::from_secs(5),
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
        .route("/auth/providers", get(providers::<S>))
        .route("/auth/logout", post(logout::<S>))
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

/// `GET`: what a login page may offer, so the frontend does not hard-code the
/// list.
///
/// # Arguments
///
/// * `state` - The application state, read for the provider registry.
#[allow(clippy::unused_async)]
async fn providers<S: AuthState>(State(state): State<S>) -> Json<Vec<ProviderInfo>> {
    Json(state.providers().info())
}

/// `POST`: exchange a credential for a session.
///
/// # Arguments
///
/// * `state` - The application state, which is where the codec, the registry
///   and the refresh store are reached.
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

/// `POST`: rotate a refresh token and mint a new session.
///
/// # Arguments
///
/// * `state` - The application state, which is where the codec, the registry
///   and the refresh store are reached.
/// * `body` - The refresh token being redeemed. It is consumed, so presenting
///   it twice fails.
async fn refresh<S: AuthState>(
    State(state): State<S>,
    Json(body): Json<RefreshRequest>,
) -> Result<Json<SessionResponse>, ApiError> {
    let tokens = state.refresh_tokens().ok_or_else(|| {
        ApiError::new(StatusCode::NOT_IMPLEMENTED, "Not Implemented")
            .with_code("REFRESH_NOT_ENABLED")
            .with_detail("this deployment does not issue refresh tokens")
    })?;

    let rotated = tokens.rotate(&body.refresh_token).await?;
    let access_token = state.sessions().issue(&rotated.principal)?;

    Ok(Json(SessionResponse {
        access_token,
        token_type: "Bearer",
        expires_in: ttl_seconds(state.sessions()),
        refresh_token: Some(rotated.token),
    }))
}

/// `POST`: revoke the caller's refresh token.
///
/// # Arguments
///
/// * `state` - The application state, which is where the codec, the registry
///   and the refresh store are reached.
/// * `body` - The token to revoke, if the client sent one. Logging out without
///   it still succeeds, because the access token expires on its own.
async fn logout<S: AuthState>(
    State(state): State<S>,
    body: Option<Json<RefreshRequest>>,
) -> Result<StatusCode, ApiError> {
    // Revoking the refresh token is what actually ends the session; the access
    // token stays valid until it expires, which is the trade ADR 0004 records.
    if let (Some(tokens), Some(Json(body))) = (state.refresh_tokens(), body) {
        tokens.revoke(&body.refresh_token).await?;
    }
    Ok(StatusCode::NO_CONTENT)
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

/// Build the session response both login and refresh return, so the two cannot
/// drift apart.
///
/// # Arguments
///
/// * `state` - The application state, for the codec and the refresh store.
/// * `principal` - Who the session is for.
async fn issue<S: AuthState>(
    state: &S,
    principal: &Principal,
) -> Result<Json<SessionResponse>, ApiError> {
    let access_token = state.sessions().issue(principal)?;
    let refresh_token = match state.refresh_tokens() {
        Some(tokens) => Some(tokens.issue(principal).await?),
        None => None,
    };

    Ok(Json(SessionResponse {
        access_token,
        token_type: "Bearer",
        expires_in: ttl_seconds(state.sessions()),
        refresh_token,
    }))
}

/// The access-token lifetime to advertise, so a client knows when to refresh
/// rather than guessing.
///
/// # Arguments
///
/// * `codec` - The codec that will issue the token, which is what owns the
///   lifetime.
fn ttl_seconds(codec: &SessionCodec) -> u64 {
    match codec {
        SessionCodec::Local(c) | SessionCodec::Either(c, _) => c.token_ttl().as_secs(),
    }
}

/// Put the caller's `Principal` in the request extensions, when they have one.
///
/// Deliberately does **not** reject an unauthenticated request: that is
/// `Authenticated<R>`'s job, in the handler signature where it is visible.
/// A layer that rejects makes every public route need an exception.
///
/// # Arguments
///
/// * `state` - The application state, which is where the codec, the registry
///   and the refresh store are reached.
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
        match state.sessions().verify(&token) {
            Ok(principal) => {
                request.extensions_mut().insert(principal);
            }
            Err(e) => {
                // An expired token has to reach the client as a 401 so it
                // knows to refresh; silently continuing anonymous would turn
                // it into a 403 from whatever came next.
                if matches!(e, AuthError::Expired) {
                    return ApiError::from(e).into_response();
                }
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
