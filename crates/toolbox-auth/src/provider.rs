//! Identity providers.
//!
//! [`IdentityProvider`] has three implementations - password, forwarded-header,
//! JWT - and is used as `dyn`, which is the whole point: a deployment picks its
//! providers at runtime and [`ProviderRegistry`] is the single place a
//! credential becomes a [`Principal`], whether it arrives at `/auth/login` or
//! on every request as a bearer token.
//!
//! A naive `AuthBackend` could not do this: it was synchronous, there was
//! exactly one per application, and `Credential` was a closed toolbox-owned
//! enum. All three are fixed here.

pub mod jwt;
#[cfg(feature = "password")]
pub mod password;
pub mod proxy_header;

use std::sync::Arc;

use async_trait::async_trait;
pub use jwt::{Claims, JwtIdentityProvider, RefreshInfo, Refreshed, TokenUse};
#[cfg(feature = "password")]
pub use password::{
    PasswordIdentityProvider, StoredUser, UserStore, auth_epoch, hash_password, verify_password,
};
pub use proxy_header::{
    ForwardedHeaders, ForwardedIdentity, ForwardedIdentityProvider, parse_network,
};

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
    /// A bearer token.
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

/// Something that can turn a credential into a principal.
#[async_trait]
pub trait IdentityProvider: Send + Sync + 'static {
    /// This provider's id: `password`, `keycloak`.
    fn id(&self) -> &str;

    /// Try to authenticate.
    ///
    /// `None` means "this credential is not mine" - a password provider hands
    /// back `None` for a bearer token rather than an error, so the registry can
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
#[derive(Default)]
pub struct ProviderRegistry {
    /// The registered providers, tried in this order.
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
    ///   [`ProviderRegistry::authenticate`] tries them in registration order,
    ///   so register the per-request bearer verifier first.
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

    /// Look one up by id.
    ///
    /// # Arguments
    ///
    /// * `id` - The provider id.
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
