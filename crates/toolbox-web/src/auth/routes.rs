//! The `/auth/*` routes and the request and response bodies they carry.
//!
//! The DTOs sit beside the handlers rather than in a module of their own: a
//! wire shape and the handler that returns it change together.

use axum::{
    Extension, Json, Router,
    extract::State,
    routing::{get, post},
};
use http::StatusCode;
use secrecy::SecretString;
use serde::{Deserialize, Serialize};
use toolbox_auth::{AuthError, Credential, Principal};

use super::{AuthState, LoginLimit, limiter::login_limiter};
use crate::error::ApiError;

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
async fn logout() -> StatusCode {
    StatusCode::NO_CONTENT
}

/// `GET`: the caller's principal, so a frontend can render its own interface
/// without re-deriving roles from the token.
///
/// # Arguments
///
/// * `principal` - What the session layer put in the extensions. Absent means
///   unauthenticated, which is a 401 here.
#[allow(clippy::unused_async)]
async fn me(principal: Option<Extension<Principal>>) -> Result<Json<Principal>, ApiError> {
    principal
        .map(|Extension(p)| Json(p))
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
