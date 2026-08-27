//! Password logins.
//!
//! It unifies argon2, the `password-hash` traits and this crate's `Principal`
//! behind one provider. There is deliberately **no** `PasswordVerifier` trait
//! here - `password_hash::PasswordVerifier` already exists, and defining a
//! second one was one of the things the review flagged.

use argon2::Argon2;
use async_trait::async_trait;
use password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng};
use secrecy::ExposeSecret as _;

use super::{Credential, IdentityProvider, ProviderInfo, ProviderKind, UserStore};
use crate::principal::{AuthError, Principal};

/// A hash of a password that no user has, used to spend the same time on an
/// unknown username as on a known one.
///
/// Without this, a failed login returns measurably faster for a username that
/// does not exist, which turns the login endpoint into a user enumerator.
const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$\
                          RdescudvJCsgt3ub+b+dWRWJTmaaJObG";

/// Hash a password for storage, in PHC format.
///
/// # Arguments
///
/// * `password` - The plaintext. It is never logged, and the returned PHC
///   string carries its own parameters so raising the cost later leaves old
///   hashes valid.
///
/// # Errors
/// [`AuthError::Malformed`] when hashing fails, which means the parameters
/// are wrong rather than the password.
pub fn hash_password(password: &str) -> Result<String, AuthError> {
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| AuthError::Malformed(e.to_string()))
}

/// Whether a password matches a stored PHC hash.
///
/// # Arguments
///
/// * `password` - The plaintext presented.
/// * `phc` - The stored PHC string. A malformed one is a failed verification,
///   never a panic.
#[must_use]
pub fn verify_password(password: &str, phc: &str) -> bool {
    PasswordHash::new(phc).is_ok_and(|parsed| {
        Argon2::default()
            .verify_password(password.as_bytes(), &parsed)
            .is_ok()
    })
}

/// Authenticates a username and password against a [`UserStore`].
pub struct PasswordProvider<S> {
    store: S,
    id: String,
    display_name: String,
}

impl<S> std::fmt::Debug for PasswordProvider<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PasswordProvider")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl<S: UserStore> PasswordProvider<S> {
    /// A provider over a user store.
    ///
    /// # Arguments
    ///
    /// * `store` - Where to look users up. The table is the consumer's, because
    ///   a toolbox that ships one has an opinion about your domain.
    pub fn new(store: S) -> Self {
        Self {
            store,
            id: "password".to_owned(),
            display_name: "Password".to_owned(),
        }
    }

    /// Override the id, for a deployment with two password realms.
    ///
    /// # Arguments
    ///
    /// * `id` - The registry id, for a deployment running two password realms
    ///   side by side.
    #[must_use]
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// Override the button label.
    ///
    /// # Arguments
    ///
    /// * `name` - What a login page shows on the button.
    #[must_use]
    pub fn display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = name.into();
        self
    }
}

#[async_trait]
impl<S: UserStore> IdentityProvider for PasswordProvider<S> {
    fn id(&self) -> &str {
        &self.id
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            kind: ProviderKind::Credential,
        }
    }

    async fn authenticate(&self, credential: &Credential) -> Option<Result<Principal, AuthError>> {
        let Credential::Password { username, password } = credential else {
            // Not ours. The registry tries the next provider.
            return None;
        };

        let found = match self.store.lookup(username).await {
            Ok(found) => found,
            Err(e) => return Some(Err(e)),
        };

        // Verify either way. Returning early for an unknown username leaks
        // which usernames exist through response time.
        let (hash, user) = match &found {
            Some(user) => (user.password_hash.as_str(), Some(user)),
            None => (DUMMY_HASH, None),
        };
        let matched = verify_password(password.expose_secret(), hash);

        match (matched, user) {
            (true, Some(user)) => Some(Ok(Principal {
                subject: user.subject.clone(),
                issuer: self.id.clone(),
                roles: user.roles.iter().cloned().collect(),
                display_name: user.display_name.clone(),
                email: user.email.clone(),
                attributes: user.attributes.clone(),
            })),
            _ => Some(Err(AuthError::Unauthenticated)),
        }
    }
}
