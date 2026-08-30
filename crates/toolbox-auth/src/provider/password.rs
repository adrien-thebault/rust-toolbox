//! Password logins.
//!
//! It unifies argon2, the `password-hash` traits and this crate's `Principal`
//! behind one provider. There is deliberately **no** `PasswordVerifier` trait
//! here - `password_hash::PasswordVerifier` already exists.

use std::collections::BTreeMap;

use argon2::Argon2;
use async_trait::async_trait;
use hmac::{Hmac, Mac};
use password_hash::{PasswordHash, PasswordHasher, PasswordVerifier, SaltString, rand_core::OsRng};
use secrecy::ExposeSecret as _;
use sha2::Sha256;

use super::{Credential, IdentityProvider};
use crate::principal::{AuthError, Principal};

/// A hash of a password that no user has, used to spend the same time on an
/// unknown username as on a known one.
///
/// Without this, a failed login returns measurably faster for a username that
/// does not exist, which turns the login endpoint into a user enumerator.
const DUMMY_HASH: &str = "$argon2id$v=19$m=19456,t=2,p=1$c29tZXNhbHRzb21lc2FsdA$\
                          RdescudvJCsgt3ub+b+dWRWJTmaaJObG";

/// Hash a password for storage, in PHC string format.
///
/// PHC (`$argon2id$v=19$m=...,t=...,p=...$<b64 salt>$<b64 digest>`) is
/// self-describing: the algorithm, its cost parameters, the salt and the
/// digest all travel in one string, so raising the cost later leaves old
/// hashes verifiable.
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

/// A stable fingerprint of a stored credential.
///
/// A stateless refresh token binds itself to this: when the password changes,
/// the PHC hash changes, so this changes, so every refresh token issued
/// against the old one stops verifying. Keyed with the session signing secret
/// so a leaked token carries nothing crackable.
///
/// # Arguments
///
/// * `signing_key` - The session signing secret, used as the HMAC key.
/// * `phc_hash` - The user's current stored PHC hash.
///
/// # Panics
/// Never: HMAC accepts a key of any length, so the keying step cannot fail.
#[must_use]
pub fn auth_epoch(signing_key: &[u8], phc_hash: &str) -> String {
    let mut mac =
        Hmac::<Sha256>::new_from_slice(signing_key).expect("HMAC takes a key of any length");
    mac.update(phc_hash.as_bytes());
    hex::encode(mac.finalize().into_bytes())
}

/// Where a password provider looks users up.
#[async_trait]
pub trait UserStore: Send + Sync + 'static {
    /// The stored PHC hash and roles for a username, if it exists.
    ///
    /// The hash is in PHC string format - `$argon2id$v=19$m=...$salt$digest` -
    /// the same shape [`hash_password`] returns.
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

/// An in-memory user store keyed by username, for a fixed set of service
/// accounts or a test fixture. A real deployment queries a table.
#[async_trait]
impl<H> UserStore for std::collections::HashMap<String, StoredUser, H>
where
    H: std::hash::BuildHasher + Send + Sync + 'static,
{
    async fn lookup(&self, username: &str) -> Result<Option<StoredUser>, AuthError> {
        Ok(self.get(username).cloned())
    }
}

/// Authenticates a username and password against a [`UserStore`].
pub struct PasswordIdentityProvider<S> {
    /// Where users are looked up.
    store: S,
    /// The registry id and `Principal::issuer` for logins this handles.
    id: String,
    /// The button label a login page shows.
    display_name: String,
}

impl<S> std::fmt::Debug for PasswordIdentityProvider<S> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PasswordIdentityProvider")
            .field("id", &self.id)
            .finish_non_exhaustive()
    }
}

impl<S: UserStore> PasswordIdentityProvider<S> {
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
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// Override the button label.
    ///
    /// # Arguments
    ///
    /// * `name` - What a login page shows on the button.
    #[must_use]
    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = name.into();
        self
    }

    /// The button label.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }
}

#[async_trait]
impl<S: UserStore> IdentityProvider for PasswordIdentityProvider<S> {
    fn id(&self) -> &str {
        &self.id
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
        let hash = found
            .as_ref()
            .map_or(DUMMY_HASH, |user| user.password_hash.as_str())
            .to_owned();
        let password = password.expose_secret().to_owned();
        // argon2 is tens of milliseconds of CPU; it does not run on the async
        // runtime.
        let matched = tokio::task::spawn_blocking(move || verify_password(&password, &hash))
            .await
            .unwrap_or(false);

        match (matched, found) {
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
