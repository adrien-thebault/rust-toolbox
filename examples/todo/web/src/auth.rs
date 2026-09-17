//! Who may log in, and what they may then do.
//!
//! The gateway owns identity and the backend trusts its caller, so everything
//! about authentication is on this side of the hop and none of it is in
//! `todo-grpc`.

use std::{collections::BTreeMap, future::Future, sync::Arc};

use async_trait::async_trait;
use secrecy::{ExposeSecret, SecretString};
use toolbox::{
    auth::{
        AuthError, ForwardedIdentityProvider, JwtIdentityProvider, PasswordIdentityProvider,
        Principal, ProviderRegistry, RefreshInfo, Role, StoredUser, UserStore, auth_epoch,
    },
    web::auth::AuthState,
};

use crate::state::AppState;

/// What the gateway needs before it can issue a session.
#[derive(Clone, clap::Args)]
pub struct AuthConfig {
    /// The session signing secret. At least 32 bytes.
    #[arg(long, env = "SESSION_SECRET", value_parser = parse_secret)]
    pub session_secret: SecretString,
    /// The `iss` claim, and the provider id sessions are stamped with.
    #[arg(long, env = "SESSION_ISSUER", default_value = "todo-web")]
    pub issuer: String,
    /// The one seeded account's username.
    #[arg(long, env = "ADMIN_USERNAME", value_parser = clap::builder::NonEmptyStringValueParser::new())]
    pub admin_username: String,
    /// That account's password, as a PHC-format argon2 hash.
    #[arg(long, env = "ADMIN_PASSWORD_HASH", value_parser = clap::builder::NonEmptyStringValueParser::new())]
    pub admin_password_hash: String,
}

/// Parse a non-empty secret without exposing it through `Debug` output.
fn parse_secret(value: &str) -> Result<SecretString, &'static str> {
    if value.is_empty() {
        Err("the secret must not be empty")
    } else {
        Ok(SecretString::from(value))
    }
}

/// The one role this example has.
///
/// A consumer defines its own; the toolbox never ships an `Admin`, because the
/// moment it does it has an opinion about your permission model.
pub struct Admin;

impl Role for Admin {
    const NAME: &'static str = "ADMIN";
}

/// A [`UserStore`] holding the single account the example seeds.
///
/// A real deployment queries a table. The point of the trait is that swapping
/// this for that one is a line in `providers`, not a change to the login route.
#[derive(Debug, Clone)]
pub struct SeededAdmin {
    /// The one account's username.
    username: String,
    /// Its PHC-format argon2 hash.
    password_hash: String,
}

impl SeededAdmin {
    /// The store described by `config`.
    ///
    /// # Arguments
    ///
    /// * `config` - Read for the username and the PHC hash. The hash is never
    ///   compared here: `PasswordIdentityProvider` verifies it either way, so a
    ///   miss and a hit take the same time and response timing cannot enumerate
    ///   accounts.
    #[must_use]
    pub fn new(config: &AuthConfig) -> Self {
        Self {
            username: config.admin_username.clone(),
            password_hash: config.admin_password_hash.clone(),
        }
    }
}

#[async_trait]
impl UserStore for SeededAdmin {
    async fn lookup(&self, username: &str) -> Result<Option<StoredUser>, AuthError> {
        if username != self.username {
            return Ok(None);
        }
        Ok(Some(StoredUser {
            subject: self.username.clone(),
            password_hash: self.password_hash.clone(),
            roles: vec![Admin::NAME.to_owned()],
            display_name: Some("Example Admin".to_owned()),
            email: None,
            attributes: BTreeMap::new(),
        }))
    }
}

/// Everything a caller may present, plus the verifier for the sessions this
/// gateway mints.
///
/// # Arguments
///
/// * `config` - Read for the seeded account.
/// * `issuer` - The session codec, registered first so a bearer token on a
///   normal request is verified before the password provider is consulted.
///
/// # Panics
/// Never in practice: the trusted-peer list is a fixed, non-empty literal.
#[must_use]
pub fn providers(config: &AuthConfig, issuer: Arc<JwtIdentityProvider>) -> ProviderRegistry {
    ProviderRegistry::new()
        .with_arc(issuer)
        .with(PasswordIdentityProvider::new(SeededAdmin::new(config)))
        // Illustrative: trusts an authenticating reverse proxy running on the
        // same host (a sidecar, or oauth2-proxy in front on localhost). A real
        // deployment behind an actual proxy would name its real peers, or use
        // `trusting_secret` when the peer address is not reliable.
        .with(
            ForwardedIdentityProvider::trusting_peers(&["127.0.0.1/32", "::1/128"])
                .expect("the trusted-peer list is a fixed, non-empty literal"),
        )
}

/// The codec that signs and verifies this gateway's sessions.
///
/// # Arguments
///
/// * `config` - Read for the signing secret and the issuer.
///
/// # Errors
/// [`AuthError`] when the secret is too short for HS256.
pub fn session_issuer(config: &AuthConfig) -> Result<JwtIdentityProvider, AuthError> {
    JwtIdentityProvider::hmac(&config.session_secret, config.issuer.clone())
}

/// The authentication accessors and refresh policy used by `auth_router`.
impl AuthState for AppState {
    fn providers(&self) -> &ProviderRegistry {
        &self.providers
    }

    fn session_issuer(&self) -> &JwtIdentityProvider {
        &self.issuer
    }

    fn refresh_epoch(&self, principal: &Principal) -> impl Future<Output = Option<String>> + Send {
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
    ) -> impl Future<Output = Result<Principal, AuthError>> + Send {
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
