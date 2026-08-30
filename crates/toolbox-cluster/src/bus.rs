//! The event bus contract, and what it promises.
//!
//! It holds state across requests, so it is a trait with adapters and declared
//! capabilities, not a struct. The capability set is the point: a feature
//! needing replay fails at subscribe time on an adapter that cannot replay, in
//! development, rather than on a Tuesday in production.

mod in_process;
mod null;

use std::{pin::Pin, time::Duration};

use async_trait::async_trait;
use futures_core::Stream;
pub use in_process::InProcessBus;
pub use null::NullBus;

use crate::event::CloudEvent;

/// A topic name. Wrapped so a topic and an arbitrary string are not the same
/// type at a call site.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Topic(String);

impl Topic {
    /// Name a topic.
    ///
    /// # Arguments
    ///
    /// * `name` - The topic name. Adapters use it verbatim, so it also has to
    ///   be legal wherever the events are stored.
    pub fn new(name: impl Into<String>) -> Self {
        Self(name.into())
    }

    /// The name.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Topic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for Topic {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

/// How many times a subscriber may see an event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Delivery {
    /// Dropped rather than redelivered. Fine for a UI notification, wrong for
    /// anything that changes state.
    AtMostOnce,
    /// Redelivered until acknowledged, so a handler must be idempotent.
    AtLeastOnce,
}

/// What ordering an adapter promises.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusOrdering {
    /// None.
    None,
    /// Events on one topic arrive in publish order.
    PerTopic,
    /// Events sharing a partition key arrive in publish order.
    PerPartitionKey,
}

/// What an adapter can actually do.
///
/// The bus contract is the **intersection** of these across adapters, so a
/// capability is declared rather than assumed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BusCapabilities {
    /// How many times a subscriber may see an event.
    pub delivery: Delivery,
    /// How far back a subscriber may resume. `None` means no replay at all.
    pub replay: Option<Duration>,
    /// What ordering is promised.
    pub ordering: BusOrdering,
    /// The largest event payload in bytes.
    pub max_payload: usize,
    /// Whether events survive a restart.
    pub durable: bool,
}

impl BusCapabilities {
    /// Reject a [`StartPosition`] this adapter cannot serve, so the failure
    /// lands at subscribe time rather than as a silently short stream.
    ///
    /// # Arguments
    ///
    /// * `from` - Where the subscriber asked to start.
    /// * `adapter` - The adapter name, for the error.
    ///
    /// # Errors
    /// [`BusError::Unsupported`] when `from` is anything but the tail and this
    /// adapter has no `replay`.
    pub fn check_start(&self, from: &StartPosition, adapter: &'static str) -> Result<(), BusError> {
        match from {
            StartPosition::Now => Ok(()),
            StartPosition::Earliest | StartPosition::Cursor(_) if self.replay.is_some() => Ok(()),
            StartPosition::Earliest | StartPosition::Cursor(_) => Err(BusError::Unsupported {
                needed: MissingCapability::Replay,
                adapter,
            }),
        }
    }

    /// Reject a payload larger than [`BusCapabilities::max_payload`], for an
    /// adapter to call in `publish` before it hands the event to its transport.
    ///
    /// # Arguments
    ///
    /// * `size` - The serialized payload size in bytes.
    ///
    /// # Errors
    /// [`BusError::TooLarge`] when `size` is over the limit.
    pub fn check_payload(&self, size: usize) -> Result<(), BusError> {
        if size > self.max_payload {
            return Err(BusError::TooLarge {
                size,
                max: self.max_payload,
            });
        }
        Ok(())
    }

    /// Reject a subscriber that needs delivery to survive a restart on an
    /// adapter that is not durable.
    ///
    /// # Arguments
    ///
    /// * `adapter` - The adapter name, for the error.
    ///
    /// # Errors
    /// [`BusError::Unsupported`] with [`MissingCapability::Durability`] when
    /// this adapter is not durable.
    pub fn require_durable(&self, adapter: &'static str) -> Result<(), BusError> {
        if self.durable {
            return Ok(());
        }
        Err(BusError::Unsupported {
            needed: MissingCapability::Durability,
            adapter,
        })
    }
}

/// A capability a caller asked for that an adapter does not have.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MissingCapability {
    /// Resuming from a cursor.
    Replay,
    /// Surviving a restart.
    Durability,
    /// Redelivery until acknowledged.
    AtLeastOnce,
}

/// Where a subscriber starts reading.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StartPosition {
    /// Only events published from now on.
    Now,
    /// Everything the adapter still holds.
    Earliest,
    /// Immediately after this cursor. Needs [`MissingCapability::Replay`].
    Cursor(String),
}

/// Why a bus operation failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum BusError {
    /// The adapter cannot do what was asked. Raised at subscribe time, not at
    /// delivery time, so the failure is visible where it can be fixed.
    #[error("the `{adapter}` event bus cannot do {needed:?}")]
    Unsupported {
        /// What was needed.
        needed: MissingCapability,
        /// Which adapter was asked.
        adapter: &'static str,
    },
    /// The payload exceeded the adapter's limit.
    #[error("event payload is {size} bytes, over the {max} byte limit")]
    TooLarge {
        /// The payload's size.
        size: usize,
        /// The limit.
        max: usize,
    },
    /// The adapter's transport failed.
    #[error("event bus transport: {0}")]
    Transport(String),
}

/// A stream of events from a subscription.
pub type EventStream = Pin<Box<dyn Stream<Item = CloudEvent> + Send>>;

/// Publish and subscribe to events.
#[async_trait]
pub trait EventBus: Send + Sync {
    /// What this adapter can do.
    fn capabilities(&self) -> BusCapabilities;

    /// Publish one event.
    ///
    /// # Arguments
    ///
    /// * `topic` - Where to publish. A topic nobody subscribes to is not an
    ///   error.
    /// * `event` - The event, envelope included. It is moved because an adapter
    ///   may need to own it past the call.
    ///
    /// # Errors
    /// [`BusError`] when the payload is too large or the transport fails.
    async fn publish(&self, topic: &Topic, event: CloudEvent) -> Result<(), BusError>;

    /// Subscribe to a topic.
    ///
    /// # Arguments
    ///
    /// * `topic` - What to subscribe to.
    /// * `from` - Where to start reading. Anything other than the tail needs a
    ///   capability, so an adapter without it fails here rather than silently
    ///   starting from now.
    ///
    /// # Errors
    /// [`BusError::Unsupported`] when `from` needs a capability this adapter
    /// does not have.
    async fn subscribe(&self, topic: &Topic, from: StartPosition) -> Result<EventStream, BusError>;
}
