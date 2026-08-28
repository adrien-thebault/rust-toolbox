//! Who may log in, and what they may then do.
//!
//! The gateway owns identity and the service trusts its caller, so everything
//! about authentication is on this side of the hop.
//!
//! Replacing the seeded account with a real one is `SeededAdmin` becoming a
//! `UserStore` over a `users` table, and one line in [`providers`]. The login
//! route does not change.

use std::sync::Arc;

use async_trait::async_trait;
use secrecy::SecretString;
use toolbox_auth::{
    AuthError, JwtCodec, PasswordProvider, ProviderRegistry, Role, SessionCodec, StoredUser,
    UserStore,
};
use toolbox_cluster::InMemoryKeyValue;

use crate::state::AppState;

/// The `iss` claim used when `SESSION_ISSUER` is unset.
///
/// A constant rather than an inline literal so the line length does not
/// depend on how long a name the project was generated with.
const DEFAULT_ISSUER: &str = "{{project-name}}";

/// The one role this project starts with.
///
/// The toolbox never ships an `Admin`: the moment it does, it has an opinion
/// about your permission model.
pub struct Admin;

impl Role for Admin {
    const NAME: &'static str = "ADMIN";
}

/// What the gateway needs before it can issue a session.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// The session signing secret. At least 32 bytes.
    pub session_secret: SecretString,
    /// The `iss` claim on every token issued.
    pub issuer: String,
    /// The one seeded account's username.
    pub admin_username: String,
    /// That account's password, as a PHC-format argon2 hash.
    pub admin_password_hash: String,
}

impl AuthConfig {
    /// Read it from the environment. `.env.example` lists exactly these.
    ///
    /// # Errors
    /// [`ConfigError`] naming the first variable that was missing.
    pub fn from_env() -> Result<Self, ConfigError> {
        Ok(Self {
            session_secret: SecretString::from(var("SESSION_SECRET")?),
            issuer: std::env::var("SESSION_ISSUER").unwrap_or_else(|_| DEFAULT_ISSUER.to_owned()),
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

/// A [`UserStore`] holding the single account this project seeds.
#[derive(Debug, Clone)]
pub struct SeededAdmin {
    username: String,
    password_hash: String,
}

impl SeededAdmin {
    /// The store described by `config`.
    ///
    /// # Arguments
    ///
    /// * `config` - Read for the username and the PHC hash. The hash is never
    ///   compared here: `PasswordProvider` verifies it either way, so a miss
    ///   and a hit take the same time and response timing cannot enumerate
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
            display_name: None,
            email: None,
            attributes: std::collections::BTreeMap::new(),
        }))
    }
}

/// Everything a caller may log in with.
///
/// # Arguments
///
/// * `config` - Read for the seeded account. Adding OIDC is one more
///   `.with(...)` here and nothing else.
#[must_use]
pub fn providers(config: &AuthConfig) -> ProviderRegistry {
    ProviderRegistry::new().with(PasswordProvider::new(SeededAdmin::new(config)))
}

/// How sessions are issued and verified.
///
/// # Arguments
///
/// * `config` - Read for the signing secret and the issuer.
///
/// # Errors
/// [`ConfigError`] when the secret is too short for HS256.
pub fn sessions(config: &AuthConfig) -> Result<SessionCodec, ConfigError> {
    Ok(SessionCodec::Local(JwtCodec::new(
        &config.session_secret,
        config.issuer.clone(),
    )?))
}

/// Assemble the state the gateway runs on.
///
/// # Arguments
///
/// * `todos` - A channel to the backend.
/// * `config` - What identity is configured from.
/// * `kv` - Where refresh tokens are kept. In process here; a clustered
///   deployment passes the PostgreSQL adapter and nothing else changes.
///
/// # Errors
/// [`ConfigError`] when the session codec or the refresh store refuses.
pub fn state(
    todos: toolbox_grpc::BackendChannel,
    config: &AuthConfig,
    kv: Arc<InMemoryKeyValue>,
) -> Result<AppState, ConfigError> {
    Ok(AppState {
        todos,
        providers: Arc::new(providers(config)),
        sessions: Arc::new(sessions(config)?),
        refresh: Some(Arc::new(toolbox_auth::RefreshTokens::new(kv)?)),
    })
}
