//! What the gateway holds, and what the auth routes read out of it.

use std::sync::Arc;

use secrecy::SecretString;
use toolbox::{
    auth::{AuthError, JwtIdentityProvider, ProviderRegistry},
    cluster::{CloudEvent, InMemoryKvStore},
    grpc::ClientChannel,
    web::{idempotency::Idempotency, realtime::Hub},
};

use crate::auth::{AuthConfig, SeededAdmin, providers, session_issuer};

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
    /// Claims `Idempotency-Key`s for the create route, backed by a `KvStore`.
    pub idempotency: Arc<Idempotency>,
    /// Fans the one upstream `WatchTodos` stream out to every connected
    /// browser. `routes::todo::forward_events` fills it; the SSE route reads it.
    pub hub: Arc<Hub<CloudEvent>>,
}

/// Assemble the state the gateway runs on.
///
/// # Errors
/// [`AuthError`] when the session issuer rejects the signing secret.
pub fn state(todos: ClientChannel, config: &AuthConfig) -> Result<AppState, AuthError> {
    let issuer = Arc::new(session_issuer(config)?);
    Ok(AppState {
        todos,
        providers: Arc::new(providers(config, Arc::clone(&issuer))),
        issuer,
        users: SeededAdmin::new(config),
        session_secret: config.session_secret.clone(),
        idempotency: Arc::new(Idempotency::new(Arc::new(InMemoryKvStore::default()))),
        hub: Arc::new(Hub::new(64)),
    })
}
