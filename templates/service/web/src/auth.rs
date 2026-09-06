//! Who may log in, and what they may then do.
//!
//! The gateway owns identity and the service trusts its caller, so everything
//! about authentication is on this side of the hop.
//!
//! Replacing the seeded account with a real one is `SeededAdmin` becoming a
//! `UserStore` over a `users` table, and one line in [`providers`]. The login
//! route does not change.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use secrecy::SecretString;
use {{crate_name}}_todo::proto::{WatchTodosRequest, todo_service_client::TodoServiceClient};
use toolbox_auth::{
    AuthError, ForwardedIdentityProvider, JwtIdentityProvider, PasswordIdentityProvider,
    ProviderRegistry, Role, StoredUser, UserStore,
};
use toolbox_cluster::{CloudEvent, InMemoryKvStore, event};
use toolbox_grpc::ClientChannel;
use toolbox_web::{
    idempotency::Idempotency,
    realtime::{Hub, HubConfig, SlowConsumer},
};
use tracing::{debug, info, warn};

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
            display_name: None,
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
/// * `config` - Read for the seeded account. Adding OIDC is one more
///   `PasswordIdentityProvider`-style `.with(...)` here and nothing else.
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
        // same host (a sidecar, or oauth2-proxy in front on localhost). Name
        // your real peers instead, or use `trusting_secret` when the peer
        // address is not reliable - or delete this if nothing sits in front.
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
        idempotency: Arc::new(Idempotency::new(Arc::new(InMemoryKvStore::default()))),
        hub: Arc::new(Hub::new(HubConfig::new(64, SlowConsumer::DropOldest))),
    })
}

/// Hold one `WatchTodos` stream open against the backend and fan every event
/// on it into `hub`, so a browser gets one SSE connection rather than each one
/// dialing the backend.
///
/// Runs for the life of the process. The stream ending - a backend restart or
/// deploy - is expected, not fatal: wait a second and reconnect, because a
/// gateway that stopped relaying would leave every browser silently stale.
///
/// # Arguments
///
/// * `todos` - The backend channel, carrying the shared secret the same as any
///   other call on it.
/// * `hub` - Where to fan the events out.
pub async fn forward_events(todos: ClientChannel, hub: Arc<Hub<CloudEvent>>) {
    loop {
        match TodoServiceClient::new(todos.channel())
            .watch_todos(WatchTodosRequest {})
            .await
        {
            Ok(response) => {
                info!("attached to the backend todo event stream");
                let mut stream = response.into_inner();
                while let Ok(Some(ev)) = stream.message().await {
                    debug!(r#type = %ev.r#type, id = ev.id, "relaying a todo event to the hub");
                    let id = ev.id;
                    match event(
                        ev.r#type,
                        {{crate_name}}_todo::EVENT_SOURCE,
                        &serde_json::json!({ "id": id }),
                    ) {
                        Ok(envelope) => {
                            hub.publish(crate::state::TODOS_TOPIC, envelope);
                        }
                        Err(e) => warn!(error = %e, id, "could not rebuild a todo event"),
                    }
                }
                info!("the backend todo event stream ended; reconnecting");
            }
            Err(status) => {
                warn!(error = %status, "could not attach to the todo event stream; retrying");
            }
        }
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}
