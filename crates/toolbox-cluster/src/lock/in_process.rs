//! Locks held in this process only.

use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_trait::async_trait;

use super::{LockCapabilities, LockError, LockGuard, LockManager, LockRelease};
use crate::deployment::{Adapter, Scope};

/// Who holds a lock, and until when.
#[derive(Debug, Clone)]
struct Lease {
    /// The owner token that took it.
    owner: String,
    /// When it lapses if not renewed.
    until: Instant,
}

/// Locks held in this process only.
///
/// **Single replica only.** Two replicas each take the "same" lock and both run
/// the work, so it declares [`Scope::Local`].
#[derive(Debug, Default)]
pub struct InProcessLocks {
    /// Every currently held lock, by key.
    held: Arc<Mutex<HashMap<String, Lease>>>,
}

impl InProcessLocks {
    /// A fresh lock table.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

/// The drop-time release hook a [`LockGuard`] calls, sharing the lock table.
struct InProcessRelease {
    /// The same table [`InProcessLocks`] holds.
    held: Arc<Mutex<HashMap<String, Lease>>>,
}

impl LockRelease for InProcessRelease {
    fn release(&self, key: &str, owner: &str) {
        let mut held = self
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Only if we still hold it: the lease may have expired and been taken
        // by someone else, and releasing then would steal their lock.
        if held.get(key).is_some_and(|h| h.owner == owner) {
            held.remove(key);
        }
    }
}

#[async_trait]
impl LockManager for InProcessLocks {
    fn capabilities(&self) -> LockCapabilities {
        LockCapabilities {
            shared: false,
            leased: true,
        }
    }

    async fn try_lock(&self, key: &str, lease: Duration) -> Result<Option<LockGuard>, LockError> {
        let owner = uuid::Uuid::now_v7().to_string();
        let now = Instant::now();
        let mut held = self
            .held
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);

        if held.get(key).is_some_and(|h| h.until > now) {
            return Ok(None);
        }
        held.insert(
            key.to_owned(),
            Lease {
                owner: owner.clone(),
                until: now + lease,
            },
        );
        drop(held);

        Ok(Some(LockGuard::new(
            key,
            owner,
            Arc::new(InProcessRelease {
                held: Arc::clone(&self.held),
            }),
        )))
    }
}

impl Adapter for InProcessLocks {
    fn name(&self) -> &'static str {
        "InProcessLocks"
    }

    fn scope(&self) -> Scope {
        Scope::Local
    }

    fn remedy(&self) -> Option<&'static str> {
        Some("set LOCK_MANAGER to a shared adapter (postgres), or run one replica")
    }
}
