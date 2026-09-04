//! The authorization extractor.
//!
//! `user.require_role(Role::Admin)?` as the first line of a handler body is a
//! check that can be forgotten, and forgetting it is silent. In the signature
//! it cannot be forgotten, it is visible in the route table, and the type
//! system checks it.

use std::marker::PhantomData;

use axum::extract::FromRequestParts;
#[cfg(feature = "auth-router")]
use axum::extract::Query;
use http::request::Parts;
#[cfg(feature = "auth-router")]
use secrecy::SecretString;
#[cfg(feature = "auth-router")]
use serde::Deserialize;
#[cfg(feature = "auth-router")]
use toolbox_auth::Credential;
use toolbox_auth::{AuthError, Principal, Role, principal::AnyRole};

#[cfg(feature = "auth-router")]
use crate::auth::AuthState;
use crate::error::ApiError;

/// A caller who is authenticated and holds `R`.
///
/// ```ignore
/// async fn delete_user(_: Authenticated<Admin>, Path(id): Path<i64>) -> Result<(), ApiError>
/// ```
///
/// The `Principal` must have been put in the request extensions by an earlier
/// layer - the session middleware, or `auth_router`. This extractor does not
/// authenticate; it requires.
#[derive(Debug, Clone)]
pub struct Authenticated<R: Role = AnyRole>(pub Principal, PhantomData<R>);

impl<R: Role> Authenticated<R> {
    /// The caller.
    #[must_use]
    pub fn principal(&self) -> &Principal {
        &self.0
    }

    /// Take the caller.
    #[must_use]
    pub fn into_principal(self) -> Principal {
        self.0
    }
}

impl<S, R> FromRequestParts<S> for Authenticated<R>
where
    S: Send + Sync,
    R: Role,
{
    type Rejection = ApiError;

    #[allow(clippy::unused_async_trait_impl)] // trait-required async signature
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        let principal = parts
            .extensions
            .get::<Principal>()
            .cloned()
            .ok_or(AuthError::Unauthenticated)?;

        if !principal.has::<R>() {
            return Err(AuthError::Forbidden {
                required: R::NAME.to_owned(),
            }
            .into());
        }
        Ok(Self(principal, PhantomData))
    }
}

/// The caller, if there is one, without requiring a role.
///
/// For an endpoint that behaves differently when signed in but does not
/// require it.
#[derive(Debug, Clone)]
pub struct MaybeAuthenticated(pub Option<Principal>);

impl<S: Send + Sync> FromRequestParts<S> for MaybeAuthenticated {
    type Rejection = std::convert::Infallible;

    #[allow(clippy::unused_async_trait_impl)] // trait-required async signature
    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(parts.extensions.get::<Principal>().cloned()))
    }
}

/// The raw query parameter, before verification.
#[cfg(feature = "auth-router")]
#[derive(Debug, Deserialize)]
struct RawToken {
    /// `?token=`.
    token: String,
}

/// A caller authenticated by a bearer token in the query string, not the
/// `Authorization` header.
///
/// For the one kind of route that cannot send a header at all - an
/// `EventSource` connection, a `WebSocket` handshake. The token is a normal
/// access token, minted with a much shorter TTL for exactly this trip
/// (`JwtIdentityProvider::issue_with_ttl`) rather than a separate credential
/// type or a separate verification path: this extractor runs the same
/// `providers().authenticate` call [`crate::auth::session_layer`] runs for
/// the header, just reading `?token=` instead.
///
/// Unlike [`Authenticated`], this does its own verification rather than
/// reading a `Principal` an earlier layer left in the request extensions -
/// there is no earlier layer for a query-string token, on purpose. Mounting
/// `session_layer` itself on an SSE route would tell every route behind it to
/// also trust `?token=`, not just that one.
#[cfg(feature = "auth-router")]
#[derive(Debug, Clone)]
pub struct QueryAuthenticated<R: Role = AnyRole>(pub Principal, PhantomData<R>);

#[cfg(feature = "auth-router")]
impl<R: Role> QueryAuthenticated<R> {
    /// The caller.
    #[must_use]
    pub fn principal(&self) -> &Principal {
        &self.0
    }

    /// Take the caller.
    #[must_use]
    pub fn into_principal(self) -> Principal {
        self.0
    }
}

#[cfg(feature = "auth-router")]
impl<S, R> FromRequestParts<S> for QueryAuthenticated<R>
where
    S: AuthState,
    R: Role,
{
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Query(raw) = Query::<RawToken>::from_request_parts(parts, state)
            .await
            .map_err(|_| AuthError::Unauthenticated)?;

        let principal = state
            .providers()
            .authenticate(&Credential::Bearer(SecretString::from(raw.token)))
            .await?;

        if !principal.has::<R>() {
            return Err(AuthError::Forbidden {
                required: R::NAME.to_owned(),
            }
            .into());
        }
        Ok(Self(principal, PhantomData))
    }
}
