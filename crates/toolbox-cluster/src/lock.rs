//! The distributed-lock contract, and what it promises.
//!
//! It holds state across requests. Every lock is **leased**: a holder that dies
//! without releasing must not block the work forever, which is the failure mode
//! that makes a scheduled job silently never run again.

mod in_process;

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
pub use in_process::InProcessLocks;

/// What a lock adapter can do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LockCapabilities {
    /// Whether the lock is visible to other replicas.
    pub shared: bool,
    /// Whether a lease expires on its own if the holder dies.
    pub leased: bool,
}

/// Why a lock operation failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum LockError {
    /// The backing store failed.
    #[error("lock manager: {0}")]
    Backend(String),
}

/// Releases a held lock. Implemented by the adapter, called from the guard.
pub trait LockRelease: Send + Sync {
    /// Release `key` if this owner still holds it.
    ///
    /// # Arguments
    ///
    /// * `key` - The lock name.
    /// * `owner` - Who is releasing it. The check is what stops a replica whose
    ///   lease already expired from releasing the lock a different replica now
    ///   holds.
    fn release(&self, key: &str, owner: &str);
}

/// A held lock. Releases on drop, unless [`LockGuard::keep`] was called.
pub struct LockGuard {
    /// The lock key.
    key: String,
    /// The owner token that holds it.
    owner: String,
    /// Where the drop-time release goes.
    manager: Arc<dyn LockRelease>,
    /// Cleared by [`LockGuard::keep`] to suppress the drop release.
    release_on_drop: bool,
}

impl LockGuard {
    /// Build a guard. Adapters call this after taking the lock.
    ///
    /// # Arguments
    ///
    /// * `key` - The lock name that was taken.
    /// * `owner` - This holder's identity, checked again at release.
    /// * `manager` - The adapter to call on drop, so the guard is one type
    ///   whatever took the lock.
    pub fn new(
        key: impl Into<String>,
        owner: impl Into<String>,
        manager: Arc<dyn LockRelease>,
    ) -> Self {
        Self {
            key: key.into(),
            owner: owner.into(),
            manager,
            release_on_drop: true,
        }
    }

    /// Hold the lock until its **lease expires**, rather than releasing on
    /// drop.
    ///
    /// For work that must happen once per *window* rather than once per
    /// caller. Releasing as soon as the work finished would let the next
    /// replica take the lock and do the same window again - which for a
    /// scheduled job means it runs once per replica instead of once.
    ///
    /// This is what ShedLock calls `lockAtLeastFor`, and it is only safe on a
    /// leased adapter: without a lease the lock would never come back.
    pub fn keep(mut self) {
        self.release_on_drop = false;
    }

    /// The key this guard holds.
    #[must_use]
    pub fn key(&self) -> &str {
        &self.key
    }
}

impl std::fmt::Debug for LockGuard {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LockGuard")
            .field("key", &self.key)
            .finish_non_exhaustive()
    }
}

impl Drop for LockGuard {
    fn drop(&mut self) {
        if self.release_on_drop {
            self.manager.release(&self.key, &self.owner);
        }
    }
}

/// Take a named lock across whatever the adapter's scope is.
#[async_trait]
pub trait LockManager: Send + Sync {
    /// What this adapter can do.
    fn capabilities(&self) -> LockCapabilities;

    /// Try to take `key` for at most `lease`.
    ///
    /// Returns `Ok(None)` when someone else holds it - not an error, because
    /// "another replica is doing it" is the expected outcome, not a fault.
    ///
    /// # Arguments
    ///
    /// * `key` - The lock name. Every replica contending must use the same one.
    /// * `lease` - How long the lock is held if the holder dies. Too short and
    ///   the work is run twice; too long and a crash blocks the job for that
    ///   duration.
    ///
    /// # Errors
    /// [`LockError::Backend`] when the store fails.
    async fn try_lock(&self, key: &str, lease: Duration) -> Result<Option<LockGuard>, LockError>;
}
