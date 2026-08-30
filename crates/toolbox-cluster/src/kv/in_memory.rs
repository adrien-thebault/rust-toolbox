//! An in-memory store over `moka`.

use std::time::{Duration, Instant};

use async_trait::async_trait;
use moka::{Expiry, future::Cache};

use super::{KvStore, KvStoreCapabilities, KvStoreError};
use crate::deployment::{Adapter, Scope};

/// An entry, carrying its own expiry so the cache can honour per-key TTLs.
#[derive(Debug, Clone)]
struct CacheEntry {
    /// The stored value.
    bytes: Vec<u8>,
    /// The TTL it was written with, if any.
    ttl: Option<Duration>,
    /// When this entry stops being valid.
    ///
    /// Kept as well as `ttl` because `take` goes through moka's `remove`,
    /// which hands back an entry that has expired but not yet been evicted.
    /// Reading `get` is filtered by moka; `remove` is not, and a `take` that
    /// returns an expired single-use token is a token that never expires.
    expires_at: Option<Instant>,
}

impl CacheEntry {
    /// Whether this entry's own expiry has passed.
    ///
    /// Checked on read as well as by `moka`: `take` goes through `remove`,
    /// which does not consult the expiry policy.
    fn is_expired(&self) -> bool {
        self.expires_at.is_some_and(|at| Instant::now() >= at)
    }
}

/// Reads each entry's own TTL rather than applying one policy to the cache.
struct PerEntryTtl;

impl Expiry<String, CacheEntry> for PerEntryTtl {
    fn expire_after_create(
        &self,
        _key: &String,
        value: &CacheEntry,
        _created_at: std::time::Instant,
    ) -> Option<Duration> {
        value.ttl
    }

    fn expire_after_update(
        &self,
        _key: &String,
        value: &CacheEntry,
        _updated_at: std::time::Instant,
        _duration_until_expiry: Option<Duration>,
    ) -> Option<Duration> {
        value.ttl
    }
}

/// An in-memory store over `moka`.
///
/// **Single replica only, but degraded rather than broken**: entries written on
/// one replica are invisible to the others, so a session or a ticket works only
/// where it was made. Bounded, so a key space an attacker controls cannot grow
/// without limit.
pub struct InMemoryKvStore {
    /// The bounded moka cache backing every operation.
    cache: Cache<String, CacheEntry>,
}

impl std::fmt::Debug for InMemoryKvStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryKvStore")
            .field("entries", &self.cache.entry_count())
            .finish()
    }
}

impl Default for InMemoryKvStore {
    fn default() -> Self {
        Self::new(100_000)
    }
}

impl InMemoryKvStore {
    /// A store holding at most `capacity` entries.
    ///
    /// # Arguments
    ///
    /// * `capacity` - The entry ceiling. `moka` evicts past it, so this bounds
    ///   the memory a store can take rather than the number of keys a caller
    ///   may use.
    #[must_use]
    pub fn new(capacity: u64) -> Self {
        Self {
            cache: Cache::builder()
                .max_capacity(capacity)
                .expire_after(PerEntryTtl)
                .build(),
        }
    }
}

#[async_trait]
impl KvStore for InMemoryKvStore {
    fn capabilities(&self) -> KvStoreCapabilities {
        KvStoreCapabilities {
            atomic_take: true,
            atomic_add: true,
            ttl: true,
            durable: false,
            shared: false,
        }
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, KvStoreError> {
        Ok(self
            .cache
            .get(key)
            .await
            .filter(|e| !e.is_expired())
            .map(|e| e.bytes))
    }

    async fn set(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Option<Duration>,
    ) -> Result<(), KvStoreError> {
        self.cache
            .insert(
                key.to_owned(),
                CacheEntry {
                    bytes: value,
                    ttl,
                    expires_at: ttl.map(|d| Instant::now() + d),
                },
            )
            .await;
        Ok(())
    }

    async fn add(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Option<Duration>,
    ) -> Result<bool, KvStoreError> {
        // An expired-but-not-evicted entry must not block a new claim, so it is
        // removed first; `remove` does not consult the expiry policy.
        if self.cache.get(key).await.is_some_and(|e| e.is_expired()) {
            self.cache.remove(key).await;
        }
        // moka runs the init future at most once per key across racing calls,
        // so exactly one caller sees `is_fresh()`.
        let entry = self
            .cache
            .entry(key.to_owned())
            .or_insert_with(async move {
                CacheEntry {
                    bytes: value,
                    ttl,
                    expires_at: ttl.map(|d| Instant::now() + d),
                }
            })
            .await;
        Ok(entry.is_fresh())
    }

    async fn take(&self, key: &str) -> Result<Option<Vec<u8>>, KvStoreError> {
        // moka's `remove` returns the previous value under the entry's lock,
        // so two concurrent takes cannot both see it - but it does *not*
        // filter an entry that has expired and not yet been evicted, so the
        // deadline is checked here.
        Ok(self
            .cache
            .remove(key)
            .await
            .filter(|e| !e.is_expired())
            .map(|e| e.bytes))
    }

    async fn delete(&self, key: &str) -> Result<(), KvStoreError> {
        self.cache.invalidate(key).await;
        Ok(())
    }
}

impl Adapter for InMemoryKvStore {
    fn name(&self) -> &'static str {
        "InMemoryKvStore"
    }

    fn scope(&self) -> Scope {
        Scope::LocalDegraded {
            note: "entries are per-process, so a value written on one replica is \
                   invisible to the others",
        }
    }

    fn remedy(&self) -> Option<&'static str> {
        Some("set KV_STORE to a shared adapter (postgres) if values must be seen by every replica")
    }
}
