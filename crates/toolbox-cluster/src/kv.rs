//! The key-value contract, and what it promises.
//!
//! It holds state across requests. The `take` capability is not decoration -
//! refresh-token rotation is built on it, and a get-then-delete race silently
//! allows exactly the replay the rotation exists to catch.

mod in_memory;

use std::time::Duration;

use async_trait::async_trait;
pub use in_memory::InMemoryKvStore;

/// What a key-value adapter can do.
///
/// Four independent flags rather than an enum: an adapter can have any
/// combination of them, and collapsing them would only hide which one a caller
/// actually needs.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct KvStoreCapabilities {
    /// Whether `take` is genuinely atomic. A caller that needs it must check,
    /// because an adapter without it turns rotation into a race.
    pub atomic_take: bool,
    /// Whether `add` is genuinely atomic. A caller that needs it must check,
    /// because an adapter without it lets two racing requests both create the
    /// same key - which is the trap an idempotency claim exists to close.
    pub atomic_add: bool,
    /// Whether per-entry expiry is honoured.
    pub ttl: bool,
    /// Whether entries survive a restart.
    pub durable: bool,
    /// Whether entries are visible to other replicas.
    pub shared: bool,
}

/// Why a key-value operation failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum KvStoreError {
    /// The adapter cannot do what was asked.
    #[error("the `{adapter}` key-value store does not support {needed}")]
    Unsupported {
        /// What was needed.
        needed: &'static str,
        /// Which adapter was asked.
        adapter: &'static str,
    },
    /// The backing store failed.
    #[error("key-value store: {0}")]
    Backend(String),
}

/// A key-value store with expiry.
#[async_trait]
pub trait KvStore: Send + Sync {
    /// What this adapter can do.
    fn capabilities(&self) -> KvStoreCapabilities;

    /// Read a key.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to read. A missing key is `Ok(None)`, not an error.
    ///
    /// # Errors
    /// [`KvStoreError::Backend`] when the store fails.
    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, KvStoreError>;

    /// Write a key, optionally expiring it.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to write. Prefix it: a shared store is shared with
    ///   every other feature.
    /// * `value` - The bytes to store. The port is untyped so one adapter
    ///   serves sessions, tickets and idempotency records alike.
    /// * `ttl` - How long it lives, or `None` to keep it until deleted.
    ///
    /// # Errors
    /// [`KvStoreError::Backend`] when the store fails.
    async fn set(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Option<Duration>,
    ) -> Result<(), KvStoreError>;

    /// Create a key only if it is not already present.
    ///
    /// **Atomic**, or the adapter must not claim
    /// [`KvStoreCapabilities::atomic_add`]. Returns `true` when this call
    /// created the entry and `false` when a live entry was already there. An
    /// expired entry counts as absent and is overwritten. This is what makes an
    /// idempotency claim safe: two racing requests cannot both create the key.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to create.
    /// * `value` - The bytes to store if the key is new.
    /// * `ttl` - How long the new entry lives, or `None` to keep it until
    ///   deleted.
    ///
    /// # Errors
    /// [`KvStoreError::Unsupported`] on an adapter without an atomic add, or
    /// [`KvStoreError::Backend`] when the store fails.
    async fn add(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Option<Duration>,
    ) -> Result<bool, KvStoreError>;

    /// Read and delete a key in one operation.
    ///
    /// **Atomic**, or the adapter must not claim
    /// [`KvStoreCapabilities::atomic_take`]. A caller uses this to make sure a value
    /// is consumed exactly once - a single-use token, a one-shot ticket - and a
    /// get-then-delete implementation lets two callers both succeed.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to read and delete in one step. This is what makes
    ///   refresh-token rotation single-use.
    ///
    /// # Errors
    /// [`KvStoreError::Unsupported`] on an adapter without atomic take, or
    /// [`KvStoreError::Backend`] when the store fails.
    async fn take(&self, key: &str) -> Result<Option<Vec<u8>>, KvStoreError>;

    /// Delete a key, whether or not it was there.
    ///
    /// # Arguments
    ///
    /// * `key` - The key to remove. Deleting a key that was never there
    ///   succeeds.
    ///
    /// # Errors
    /// [`KvStoreError::Backend`] when the store fails.
    async fn delete(&self, key: &str) -> Result<(), KvStoreError>;
}
