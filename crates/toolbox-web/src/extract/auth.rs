//! The authorization extractor.
//!
//! `user.require_role(Role::Admin)?` as the first line of a handler body is a
//! check that can be forgotten, and forgetting it is silent. In the signature
//! it cannot be forgotten, it is visible in the route table, and the type
//! system checks it.

use std::marker::PhantomData;

use axum::extract::FromRequestParts;
use http::request::Parts;
use toolbox_auth::{AuthError, Principal, Role, principal::AnyRole};

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

    async fn from_request_parts(parts: &mut Parts, _state: &S) -> Result<Self, Self::Rejection> {
        Ok(Self(parts.extensions.get::<Principal>().cloned()))
    }
}
