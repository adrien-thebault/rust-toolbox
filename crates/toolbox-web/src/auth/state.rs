//! The trait the auth routes and middleware read the application state through.

use std::future::{self, Future};

use toolbox_auth::{AuthError, JwtIdentityProvider, Principal, ProviderRegistry, RefreshInfo};

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
    /// stateless baseline, `None`, issues refresh tokens with no credential
    /// binding and relies on the refresh TTL as the revocation window.
    ///
    /// # Arguments
    ///
    /// * `_principal` - Who the refresh token will be for, fresh from login.
    fn refresh_epoch(&self, _principal: &Principal) -> impl Future<Output = Option<String>> + Send {
        future::ready(None)
    }

    /// Re-resolve a principal when a refresh token is redeemed.
    ///
    /// Given what the token carried ([`RefreshInfo`]), return the principal
    /// **as it is now** - re-read your user store so a demotion or a disabled
    /// account takes effect on the next refresh, not after the full refresh
    /// TTL. Return `Err(AuthError::Unauthenticated)` to reject: the account is
    /// gone, or `info.epoch` no longer matches the stored credential. The
    /// stateless baseline trusts the token as-is.
    fn resolve_refresh(
        &self,
        info: RefreshInfo,
    ) -> impl Future<Output = Result<Principal, AuthError>> + Send {
        future::ready(Ok(info.stale))
    }
}
