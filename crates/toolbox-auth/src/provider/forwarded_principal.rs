//! Trusting a principal your own gateway already resolved.
//!
//! [`ForwardedIdentityProvider`](super::proxy_header::ForwardedIdentityProvider)
//! trusts a reverse proxy's raw headers; this trusts a whole [`Principal`] the
//! gateway built from a validated session and passed on. There is no signature
//! and no peer check: authenticity is established one layer out, by the
//! transport's shared-secret gate.
//!
//! # Read this before enabling it
//!
//! Without a shared-secret gate in front, anything that can reach the service
//! can assert any principal. `toolbox-grpc`'s `shared_secret_layer` is that
//! gate and is mandatory; this type has no way to check for it, so wiring it
//! alone is a total authentication bypass.

use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::STANDARD};

use super::{Credential, IdentityProvider};
use crate::principal::{AuthError, Principal};

/// A [`Principal`] carried across a service boundary, as the gateway encoded
/// it.
///
/// It is exactly a [`Principal`]: the wrapper exists so the transport has a
/// distinct type to put in [`Credential::Custom`], separate from a
/// proxy-header [`ForwardedIdentity`](super::proxy_header::ForwardedIdentity).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardedPrincipal(pub Principal);

impl From<&Principal> for ForwardedPrincipal {
    fn from(principal: &Principal) -> Self {
        Self(principal.clone())
    }
}

impl ForwardedPrincipal {
    /// Encode for a transport header: the principal's JSON, base64, ASCII-safe.
    #[must_use]
    pub fn encode(&self) -> String {
        // `Principal` is plain `Serialize` data, so `to_vec` cannot fail; the
        // empty fallback would only fail the matching `decode`, never panic.
        // Its JSON can carry arbitrary UTF-8 in `display_name` or `attributes`,
        // so base64 keeps the header ASCII.
        STANDARD.encode(serde_json::to_vec(&self.0).unwrap_or_default())
    }

    /// Decode what [`ForwardedPrincipal::encode`] produced.
    ///
    /// # Arguments
    ///
    /// * `encoded` - The header value: base64 of a principal's JSON.
    ///
    /// # Errors
    /// [`AuthError::Malformed`] when the value is not base64 of a principal.
    pub fn decode(encoded: &str) -> Result<Self, AuthError> {
        let bytes = STANDARD
            .decode(encoded)
            .map_err(|_| AuthError::Malformed("forwarded principal is not base64".to_owned()))?;
        let principal = serde_json::from_slice(&bytes).map_err(|_| {
            AuthError::Malformed("forwarded principal is not a principal".to_owned())
        })?;
        Ok(Self(principal))
    }
}

/// Turns a gateway-forwarded [`ForwardedPrincipal`] back into a [`Principal`].
///
/// A plain resolver: it trusts its input, because the transport's shared-secret
/// gate already established that the caller is the gateway. See the module
/// docs.
#[derive(Debug, Clone)]
pub struct ForwardedPrincipalProvider {
    /// The registry id.
    id: String,
}

impl Default for ForwardedPrincipalProvider {
    fn default() -> Self {
        Self {
            id: "forwarded-principal".to_owned(),
        }
    }
}

impl ForwardedPrincipalProvider {
    /// A provider with the default id.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Override the id.
    ///
    /// # Arguments
    ///
    /// * `id` - The registry id for this provider.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }
}

#[async_trait]
impl IdentityProvider for ForwardedPrincipalProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn authenticate(&self, credential: &Credential) -> Option<Result<Principal, AuthError>> {
        // The identity arrives as forwarded metadata, not a posted credential,
        // so the transport wraps it in `Credential::Custom`.
        let Credential::Custom(any) = credential else {
            return None;
        };
        let forwarded = any.downcast_ref::<ForwardedPrincipal>()?;
        if forwarded.0.subject.is_empty() {
            return Some(Err(AuthError::Unauthenticated));
        }
        Some(Ok(forwarded.0.clone()))
    }
}
