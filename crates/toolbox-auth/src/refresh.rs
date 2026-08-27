//! Refresh tokens, rotated on every use.
//!
//! A refresh token is a key with a TTL, so this is a struct over the existing
//! [`KeyValueStore`] rather than a second trait describing the same thing,
//! which would have needed its own local and shared adapters duplicating the
//! first's.
//!
//! Rotation is built on that trait's **atomic** `take`. A get-then-delete
//! implementation lets two callers both redeem the same token, which is
//! exactly the replay the rotation exists to catch.

use std::{sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use toolbox_cluster::KeyValueStore;
use tracing::info;

use crate::principal::{AuthError, Principal};

/// How long a refresh token lives by default.
pub const DEFAULT_TTL: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// The key prefix, so refresh tokens cannot collide with anything else in a
/// shared store.
const PREFIX: &str = "toolbox:refresh:";

/// What a refresh token resolves to in the store.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct TokenRecord {
    principal: Principal,
    /// Which family this token belongs to, so revoking one revokes the chain.
    family: String,
}

/// A newly issued refresh token and the principal it stands for.
#[derive(Debug, Clone)]
pub struct IssuedToken {
    /// The opaque token to hand the client. Never logged.
    pub token: String,
    /// Who it authenticates.
    pub principal: Principal,
}

/// Issues and rotates refresh tokens over the key-value port.
pub struct RefreshTokens {
    kv: Arc<dyn KeyValueStore>,
    ttl: Duration,
}

impl std::fmt::Debug for RefreshTokens {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RefreshTokens")
            .field("ttl", &self.ttl)
            .finish_non_exhaustive()
    }
}

impl RefreshTokens {
    /// Build over a key-value store.
    ///
    /// # Arguments
    ///
    /// * `kv` - The store. It must declare an atomic take, or rotation cannot
    ///   detect a replayed token, so a store without it is refused here rather
    ///   than at the first rotation.
    ///
    /// # Errors
    /// [`AuthError::Malformed`] when the adapter cannot promise an atomic
    /// take. Refusing here rather than at runtime is the point of declaring
    /// capabilities: a store that cannot do this silently permits replay.
    pub fn new(kv: Arc<dyn KeyValueStore>) -> Result<Self, AuthError> {
        if !kv.capabilities().atomic_take {
            return Err(AuthError::Malformed(
                "refresh tokens need a key-value store with an atomic take".to_owned(),
            ));
        }
        Ok(Self {
            kv,
            ttl: DEFAULT_TTL,
        })
    }

    /// How long an issued token lives.
    ///
    /// # Arguments
    ///
    /// * `ttl` - How long a token lives. It is the window a stolen token stays
    ///   useful, so it trades re-login frequency against exposure.
    #[must_use]
    pub fn ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Issue a fresh token for a principal.
    ///
    /// # Arguments
    ///
    /// * `principal` - Who the token will authenticate. It starts a new family,
    ///   so revoking it later revokes only this chain.
    ///
    /// # Errors
    /// [`AuthError::Malformed`] when the store fails.
    pub async fn issue(&self, principal: &Principal) -> Result<String, AuthError> {
        self.store(principal, &new_token()).await
    }

    /// Redeem a token and issue its replacement.
    ///
    /// The old token is consumed atomically, so presenting it twice fails the
    /// second time - which is how a leaked token is noticed at all.
    ///
    /// # Arguments
    ///
    /// * `token` - The token being redeemed. It is consumed atomically, so a
    ///   second presentation fails and a leak is visible.
    ///
    /// # Errors
    /// [`AuthError::Unauthenticated`] when the token is unknown or already
    /// consumed.
    pub async fn rotate(&self, token: &str) -> Result<IssuedToken, AuthError> {
        let raw = self
            .kv
            .take(&key(token))
            .await
            .map_err(|e| AuthError::Malformed(e.to_string()))?
            .ok_or_else(|| {
                // Either it never existed, expired, or was already redeemed.
                // The third case means it leaked, and there is no way to tell
                // the three apart from here - which is why the family exists.
                info!("a refresh token was presented that is not valid");
                AuthError::Unauthenticated
            })?;

        let record: TokenRecord =
            serde_json::from_slice(&raw).map_err(|e| AuthError::Malformed(e.to_string()))?;

        let next = self.store(&record.principal, &record.family).await?;
        Ok(IssuedToken {
            token: next,
            principal: record.principal,
        })
    }

    /// Revoke a token.
    ///
    /// # Arguments
    ///
    /// * `token` - The token to drop. Revoking one that was never there
    ///   succeeds.
    ///
    /// # Errors
    /// [`AuthError::Malformed`] when the store fails.
    pub async fn revoke(&self, token: &str) -> Result<(), AuthError> {
        self.kv
            .delete(&key(token))
            .await
            .map_err(|e| AuthError::Malformed(e.to_string()))
    }

    /// Mint a token and write its record, which is what both issue and rotate
    /// do.
    ///
    /// # Arguments
    ///
    /// * `principal` - Who the new token authenticates.
    /// * `family` - The chain it belongs to, carried across every rotation so
    ///   the whole chain can be revoked at once.
    async fn store(&self, principal: &Principal, family: &str) -> Result<String, AuthError> {
        let token = new_token();
        let value = serde_json::to_vec(&TokenRecord {
            principal: principal.clone(),
            family: family.to_owned(),
        })
        .map_err(|e| AuthError::Malformed(e.to_string()))?;

        self.kv
            .set(&key(&token), value, Some(self.ttl))
            .await
            .map_err(|e| AuthError::Malformed(e.to_string()))?;
        Ok(token)
    }
}

/// The store key for a token, prefixed so refresh tokens cannot collide with
/// anything else in a shared store.
///
/// # Arguments
///
/// * `token` - The opaque token.
fn key(token: &str) -> String {
    format!("{PREFIX}{token}")
}

/// A 256-bit opaque token, hex-encoded.
///
/// Opaque on purpose: a refresh token carries no claims, so there is nothing
/// in it to read, and nothing that stays true if the principal changes.
/// Straight from the OS source rather than from a UUID: it is a secret looked
/// up by exact key, so it wants entropy and nothing else.
fn new_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS random source");
    bytes.iter().fold(String::with_capacity(64), |mut out, b| {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
        out
    })
}
