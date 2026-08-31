//! Who may log in, and what they may then do.
//!
//! The gateway owns identity and the backend trusts its caller, so everything
//! about authentication is on this side of the hop and none of it is in
//! `example-todo`.

use std::sync::Arc;

use async_trait::async_trait;
use secrecy::SecretString;
use toolbox_auth::{
    AuthError, JwtIdentityProvider, PasswordIdentityProvider, ProviderRegistry, Role, StoredUser,
    UserStore,
};

use crate::state::AppState;

/// The one role this example has.
///
/// A consumer defines its own; the toolbox never ships an `Admin`, because the
/// moment it does it has an opinion about your permission model.
pub struct Admin;

impl Role for Admin {
    const NAME: &'static str = "ADMIN";
}

/// What the gateway needs before it can issue a session.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// The session signing secret. At least 32 bytes.
    pub session_secret: SecretString,
    /// The `iss` claim, and the provider id sessions are stamped with.
    pub issuer: String,
    /// The one seeded account's username.
    pub admin_username: String,
    /// That account's password, as a PHC-format argon2 hash.
    pub admin_password_hash: String,
}

impl AuthConfig {
    /// Read it from the environment, which is what a deployment does.
    ///
    /// This is the composition root: `.env.example` lists exactly these four
    /// variables, and `./scripts/hash-password.sh` produces the fourth.
    ///
    /// # Errors
    /// [`ConfigError`] naming the first variable that was missing.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            session_secret: SecretString::from(var("SESSION_SECRET")?),
            issuer: std::env::var("SESSION_ISSUER").unwrap_or_else(|_| "example-web".to_owned()),
            admin_username: var("ADMIN_USERNAME")?,
            admin_password_hash: var("ADMIN_PASSWORD_HASH")?,
        })
    }
}

/// One required variable, or the error naming it.
///
/// # Arguments
///
/// * `name` - The variable to read. Missing and empty are the same failure: an
///   empty `SESSION_SECRET` is a worse outcome than refusing to start.
fn var(name: &'static str) -> Result<String, ConfigError> {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => Ok(value),
        _ => Err(ConfigError::Missing(name)),
    }
}

/// The gateway could not be configured.
#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    /// A required variable was unset or empty.
    #[error("{0} is not set")]
    Missing(&'static str),
    /// The session codec refused the secret.
    #[error(transparent)]
    Session(#[from] AuthError),
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
            attributes: std::collections::BTreeMap::new(),
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
#[must_use]
pub fn providers(config: &AuthConfig, issuer: Arc<JwtIdentityProvider>) -> ProviderRegistry {
    ProviderRegistry::new()
        .with_arc(issuer)
        .with(PasswordIdentityProvider::new(SeededAdmin::new(config)))
}

/// The codec that signs and verifies this gateway's sessions.
///
/// # Arguments
///
/// * `config` - Read for the signing secret and the issuer.
///
/// # Errors
/// [`ConfigError`] when the secret is too short for HS256.
pub fn session_issuer(config: &AuthConfig) -> Result<JwtIdentityProvider, ConfigError> {
    Ok(JwtIdentityProvider::hmac(
        &config.session_secret,
        config.issuer.clone(),
    )?)
}

/// Assemble the state the gateway runs on.
///
/// # Arguments
///
/// * `todos` - A channel to the backend.
/// * `config` - What identity is configured from.
///
/// # Errors
/// [`ConfigError`] when the session codec refuses the secret.
pub fn state(
    todos: toolbox_grpc::ClientChannel,
    config: &AuthConfig,
) -> Result<AppState, ConfigError> {
    let issuer = Arc::new(session_issuer(config)?);
    Ok(AppState {
        todos,
        providers: Arc::new(providers(config, Arc::clone(&issuer))),
        issuer,
        users: SeededAdmin::new(config),
        session_secret: config.session_secret.clone(),
    })
}
