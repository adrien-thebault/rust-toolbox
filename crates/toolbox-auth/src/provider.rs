//! Identity providers.
//!
//! The trait has several implementations - password, proxy-header, OIDC - and
//! is used as `dyn`, which is the whole point: a deployment picks its providers
//! at runtime, and the login page renders whatever is registered without a
//! frontend change.
//!
//! A naive `AuthBackend` could not do OIDC for three structural
//! reasons: it was synchronous, there was exactly one per application, and
//! `Credential` was a closed toolbox-owned enum. All three are fixed here.

pub mod claims_mapping;
#[cfg(feature = "oidc")]
pub mod oidc;
#[cfg(feature = "password")]
pub mod password;
pub mod proxy_header;

use std::{collections::BTreeMap, sync::Arc};

use async_trait::async_trait;
pub use claims_mapping::{ClaimPath, ClaimsMapping};
#[cfg(feature = "oidc")]
pub use oidc::{AuthSession, OidcProvider};
#[cfg(feature = "password")]
pub use password::{PasswordProvider, hash_password, verify_password};
pub use proxy_header::{ForwardedIdentity, ProxyHeaderProvider, parse_network};
use serde::Serialize;

use crate::principal::{AuthError, Principal};

/// What a caller presented.
///
/// `#[non_exhaustive]` and with a `Custom` escape, so a provider the toolbox
/// does not know about can define its own credential without a release here.
#[non_exhaustive]
pub enum Credential {
    /// A username and a password.
    Password {
        /// The username.
        username: String,
        /// The password, unhashed. Never logged, never stored.
        password: secrecy::SecretString,
    },
    /// A long-lived API key.
    ApiKey(secrecy::SecretString),
    /// A bearer token from somewhere else.
    Bearer(secrecy::SecretString),
    /// Anything a provider outside the toolbox defines.
    Custom(Box<dyn std::any::Any + Send + Sync>),
}

impl std::fmt::Debug for Credential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the secret. A Debug of a login request ends up in a log.
        f.write_str(match self {
            Self::Password { .. } => "Credential::Password(<redacted>)",
            Self::ApiKey(_) => "Credential::ApiKey(<redacted>)",
            Self::Bearer(_) => "Credential::Bearer(<redacted>)",
            Self::Custom(_) => "Credential::Custom",
        })
    }
}

/// What a login page needs to render a provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ProviderInfo {
    /// The provider's id, used in its callback URL.
    pub id: String,
    /// What to call it on a button.
    pub display_name: String,
    /// How a client starts a login with it.
    pub kind: ProviderKind,
}

/// How a login with this provider begins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    /// Post a credential to `/auth/login`.
    Credential,
    /// Redirect the browser to `/auth/oidc/{id}`.
    Redirect,
}

/// Something that can turn a credential into a principal.
#[async_trait]
pub trait IdentityProvider: Send + Sync + 'static {
    /// This provider's id: `password`, `keycloak`.
    fn id(&self) -> &str;

    /// What a login page needs to render it.
    fn info(&self) -> ProviderInfo;

    /// Try to authenticate.
    ///
    /// `None` means "this credential is not mine" - a password provider hands
    /// back `None` for an API key rather than an error, so the registry can
    /// try the next provider.
    ///
    /// # Arguments
    ///
    /// * `_credential` - What the caller presented. Returning `None` means this
    ///   credential belongs to another provider, which is how several providers
    ///   coexist.
    async fn authenticate(&self, _credential: &Credential) -> Option<Result<Principal, AuthError>> {
        None
    }
}

/// Every provider a deployment has enabled.
///
/// Several at once, which is what a naive one-backend-per-app design
/// made impossible: running password logins and Keycloak side by side during a
/// migration is the normal case, not an exotic one.
#[derive(Default)]
pub struct ProviderRegistry {
    providers: Vec<Arc<dyn IdentityProvider>>,
}

impl std::fmt::Debug for ProviderRegistry {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ProviderRegistry")
            .field("providers", &self.ids())
            .finish()
    }
}

impl ProviderRegistry {
    /// An empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a provider.
    ///
    /// # Arguments
    ///
    /// * `provider` - The provider to register. Order matters:
    ///   [`ProviderRegistry::authenticate`] tries them in registration order.
    #[must_use]
    pub fn with(mut self, provider: impl IdentityProvider) -> Self {
        self.providers.push(Arc::new(provider));
        self
    }

    /// Register an already-shared provider.
    ///
    /// # Arguments
    ///
    /// * `provider` - An already-shared provider, for one the caller also holds
    ///   a handle to.
    #[must_use]
    pub fn with_arc(mut self, provider: Arc<dyn IdentityProvider>) -> Self {
        self.providers.push(provider);
        self
    }

    /// The registered ids, in order.
    #[must_use]
    pub fn ids(&self) -> Vec<&str> {
        self.providers.iter().map(|p| p.id()).collect()
    }

    /// What a login page needs, for every provider.
    ///
    /// This endpoint is small and decouples a lot: adding Keycloak becomes a
    /// deployment change rather than a frontend release.
    #[must_use]
    pub fn info(&self) -> Vec<ProviderInfo> {
        self.providers.iter().map(|p| p.info()).collect()
    }

    /// Look one up by id.
    ///
    /// # Arguments
    ///
    /// * `id` - The provider id, as a login page received it from `GET
    ///   /auth/providers`.
    #[must_use]
    pub fn get(&self, id: &str) -> Option<&Arc<dyn IdentityProvider>> {
        self.providers.iter().find(|p| p.id() == id)
    }

    /// Try every provider in order until one claims the credential.
    ///
    /// # Arguments
    ///
    /// * `credential` - What the caller presented. The first provider to claim
    ///   it decides, so a later provider never sees it.
    ///
    /// # Errors
    /// The first provider's error, or [`AuthError::Unauthenticated`] when none
    /// claimed it.
    pub async fn authenticate(&self, credential: &Credential) -> Result<Principal, AuthError> {
        for provider in &self.providers {
            if let Some(result) = provider.authenticate(credential).await {
                return result;
            }
        }
        Err(AuthError::Unauthenticated)
    }

    /// Whether anything is registered.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.providers.is_empty()
    }
}

/// Where a password provider looks users up.
#[async_trait]
pub trait UserStore: Send + Sync + 'static {
    /// The stored PHC hash and roles for a username, if it exists.
    ///
    /// # Arguments
    ///
    /// * `username` - The identifier the caller typed. A miss must take the
    ///   same time as a hit, or the response time enumerates accounts.
    ///
    /// # Errors
    /// [`AuthError`] when the store fails.
    async fn lookup(&self, username: &str) -> Result<Option<StoredUser>, AuthError>;
}

/// What a user store returns.
#[derive(Debug, Clone)]
pub struct StoredUser {
    /// The stable subject.
    pub subject: String,
    /// A PHC-format password hash.
    pub password_hash: String,
    /// The roles this user holds.
    pub roles: Vec<String>,
    /// Display name.
    pub display_name: Option<String>,
    /// Email.
    pub email: Option<String>,
    /// Anything else.
    pub attributes: BTreeMap<String, String>,
}
