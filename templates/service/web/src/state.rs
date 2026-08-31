//! What the gateway holds, and what the auth routes read out of it.

use std::sync::Arc;

use secrecy::{ExposeSecret, SecretString};
use toolbox_auth::{
    AuthError, JwtIdentityProvider, Principal, ProviderRegistry, RefreshInfo, UserStore, auth_epoch,
};
use toolbox_grpc::ClientChannel;
use toolbox_web::auth::AuthState;

use crate::auth::SeededAdmin;

/// Everything a handler can reach.
#[derive(Clone)]
pub struct AppState {
    /// A channel to the backend.
    pub todos: ClientChannel,
    /// Everything a caller may present, including the bearer verifier.
    pub providers: Arc<ProviderRegistry>,
    /// The codec that mints this gateway's sessions.
    pub issuer: Arc<JwtIdentityProvider>,
    /// The user store, for re-fingerprinting a credential on refresh.
    pub users: SeededAdmin,
    /// The signing secret, keyed into the credential fingerprint.
    pub session_secret: SecretString,
}

/// The accessors `auth_router` needs. Implementing this is what mounts login,
/// refresh, logout and `/auth/me` without writing any of them.
impl AuthState for AppState {
    fn providers(&self) -> &ProviderRegistry {
        &self.providers
    }

    fn session_issuer(&self) -> &JwtIdentityProvider {
        &self.issuer
    }

    fn refresh_epoch(
        &self,
        principal: &Principal,
    ) -> impl std::future::Future<Output = Option<String>> + Send {
        // Bind the refresh token to the stored credential: a password change
        // re-fingerprints, so every refresh token issued against the old hash
        // stops verifying.
        let secret = self.session_secret.clone();
        let users = self.users.clone();
        let subject = principal.subject.clone();
        async move {
            let user = users.lookup(&subject).await.ok().flatten()?;
            Some(auth_epoch(
                secret.expose_secret().as_bytes(),
                &user.password_hash,
            ))
        }
    }

    fn resolve_refresh(
        &self,
        info: RefreshInfo,
    ) -> impl std::future::Future<Output = Result<Principal, AuthError>> + Send {
        // Re-read the user so roles and account status are current, and reject
        // if the bound credential fingerprint no longer matches.
        let secret = self.session_secret.clone();
        let users = self.users.clone();
        async move {
            let Some(user) = users
                .lookup(&info.subject)
                .await
                .map_err(|_| AuthError::Unauthenticated)?
            else {
                return Err(AuthError::Unauthenticated);
            };
            if let Some(bound) = info.epoch.as_deref()
                && bound != auth_epoch(secret.expose_secret().as_bytes(), &user.password_hash)
            {
                return Err(AuthError::Unauthenticated);
            }
            Ok(Principal {
                subject: user.subject,
                issuer: info.idp,
                roles: user.roles.into_iter().collect(),
                display_name: user.display_name,
                email: user.email,
                attributes: user.attributes,
            })
        }
    }
}
