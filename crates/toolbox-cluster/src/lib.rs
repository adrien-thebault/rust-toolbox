//! The multi-replica seam.
//!
//! [`event`], [`kv`] and [`lock`] each hold state across requests, so each is a
//! trait with adapters - a local one, at least one shared one. The CloudEvents
//! envelope rides along in [`event`] because `toolbox-core` takes no
//! dependencies and it has nowhere smaller to live.
//!
//! Two things are deliberately absent. There is no `RateLimitStore`: with the
//! local limiter staying in `toolbox-web`, that trait would have zero
//! implementations. There is no `RefreshTokenStore` either: a refresh token is
//! a key with a TTL, so `toolbox-auth` builds one over [`kv::KvStore`] and
//! inherits every adapter for free.

pub mod event;
pub mod kv;
pub mod lock;

pub use event::{
    CloudEvent, EventBus, EventBusError, EventError, EventStream, InProcessEventBus, Topic, event,
    payload, signal,
};
pub use kv::{InMemoryKvStore, KvStore, KvStoreError};
pub use lock::{InProcessLockManager, LockGuard, LockManager, LockManagerError};
