//! authentication and RBAC.
//!
//! [`AuthBackend`] verifies a [`Credential`] and returns a [`User`].
//! [`InMemoryBasicAuthBackend`] is the only implementation ([`Credential::Basic`]
//! checked against an in-memory user map).
//!
//! [`JwtCodec`] (in [`jwt`]) issues/verifies the signed-token session,
//! independently of whichever [`AuthBackend`] produced the [`User`].
//!
//! `User::roles` is `Vec<String>`, not an enum - each consuming service
//! defines its own role type and converts to/from `&str` at the boundary
//! (see [`User::require_role`]).

use crate::axum_tools::api_error::ApiError;
use axum::{extract::FromRequestParts, http::request::Parts};
use serde_derive::{Deserialize, Serialize};
use thiserror::Error;

/// JWT-backed session issuance/verification
mod jwt;
pub use jwt::*;

/// the in-memory, fixed-user-set [`AuthBackend`] implementation
mod in_memory;
pub use in_memory::*;

/// an authenticated principal. Also an axum extractor (see the
/// `FromRequestParts` impl below): a route handler can take a `User`
/// parameter directly to require a valid session.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct User {
    /// this user's identifier, e.g. a username
    pub subject: String,
    /// the roles held by this user, e.g. `"admin"`
    pub roles: Vec<String>,
}

impl User {
    /// fails with [`AuthError::InsufficientRole`] unless this user holds `role`
    pub fn require_role(&self, role: impl AsRef<str>) -> Result<(), AuthError> {
        if self.roles.iter().any(|r| r == role.as_ref()) {
            Ok(())
        } else {
            Err(AuthError::InsufficientRole)
        }
    }
}

/// credentials presented to an [`AuthBackend`]. A backend that doesn't
/// support a given variant rejects it with
/// [`AuthError::UnsupportedCredential`].
#[derive(Debug, Clone)]
pub enum Credential {
    /// a plain username/password pair
    Basic {
        /// the presented username
        username: String,
        /// the presented password
        password: String,
    },
}

/// errors from [`AuthBackend::authenticate`]/[`User::require_role`]
#[derive(Debug, Error)]
pub enum AuthError {
    /// the presented [`Credential`] didn't match any known identity
    #[error("invalid credentials")]
    InvalidCredentials,
    /// the [`AuthBackend`] doesn't handle this [`Credential`] variant
    #[error("credential type not supported by this backend")]
    UnsupportedCredential,
    /// the [`User`] doesn't hold the role required by [`User::require_role`]
    #[error("insufficient role")]
    InsufficientRole,
}

/// verifies a [`Credential`] and returns the [`User`] it identifies.
///
/// Synchronous: implementations are expected to do a local check (e.g. a
/// hashed password comparison, or a signature check against a cached key
/// set), not an async call such as remote token introspection.
pub trait AuthBackend: Send + Sync {
    /// verifies `credential` and returns the [`User`] it identifies
    fn authenticate(&self, credential: Credential) -> Result<User, AuthError>;
}

/// axum extractor: reads the `Authorization: Bearer <token>` header,
/// decodes the session, and yields the [`User`] it identifies - or a 401,
/// as the same `application/problem+json` body every other [`ApiError`]
/// produces. The auth scheme is matched case-insensitively per
/// [RFC 7235 §2.1](https://www.rfc-editor.org/rfc/rfc7235#section-2.1).
impl<S> FromRequestParts<S> for User
where
    S: JwtCodecProvider + Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let token = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .and_then(|value| {
                let (scheme, token) = value.split_once(' ')?;
                scheme
                    .eq_ignore_ascii_case("bearer")
                    .then(|| token.trim_start())
            })
            .ok_or(ApiError::Unauthenticated)?;

        state
            .jwt_codec()
            .decode(token)
            .map(Self::from)
            .map_err(ApiError::from)
    }
}
