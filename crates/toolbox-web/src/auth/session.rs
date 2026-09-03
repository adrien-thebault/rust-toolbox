//! The session middleware.

use axum::{
    extract::{Request, State},
    middleware::Next,
    response::{IntoResponse, Response},
};
use http::header::AUTHORIZATION;
use secrecy::SecretString;
use toolbox_auth::{AuthError, Credential};
use tracing::debug;

use super::AuthState;
use crate::error::ApiError;

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
    mut request: Request,
    next: Next,
) -> Response {
    let token = request
        .headers()
        .get(AUTHORIZATION)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "))
        .map(str::to_owned);

    if let Some(token) = token {
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
