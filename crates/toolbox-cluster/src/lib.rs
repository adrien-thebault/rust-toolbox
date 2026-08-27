//! The multi-replica seam.
//!
//! Everything here holds state across requests, so each one is a trait with
//! adapters: a local one, at least one shared one, and capabilities declared
//! rather than assumed. The guard in [`deployment`] turns "this only
//! works on one replica" from something you find out in production into a
//! startup error.
//!
//! Two things are deliberately absent. There is no `RateLimitStore`: with the
//! local limiter staying in `toolbox-web`, that trait would have zero
//! implementations. There is no `RefreshTokenStore` either: a refresh token is
//! a key with a TTL, so `toolbox-auth` builds one over [`key_value::KeyValueStore`]
//! and inherits every adapter for free.

pub mod bus;
pub mod clock;
pub mod deployment;
pub mod event;
pub mod key_value;
pub mod lock;

pub use bus::{
    BusCapabilities, BusError, BusOrdering, Delivery, EventBus, EventStream, InProcessBus,
    MissingCapability, NullBus, StartPosition, Topic,
};
pub use clock::{Clock, ManualClock, SystemClock};
pub use deployment::{Adapter, Deployment, DeploymentError, Scope, check_deployment};
pub use event::{CloudEvent, EventError, event, payload, signal};
pub use key_value::{InMemoryKeyValue, KeyValueCapabilities, KeyValueError, KeyValueStore};
pub use lock::{InProcessLocks, LockCapabilities, LockError, LockGuard, LockManager};
