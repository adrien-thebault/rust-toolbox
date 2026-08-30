//! The CloudEvents envelope and the contract for moving it between replicas.
//!
//! The envelope is CloudEvents 1.0 from the
//! [official Rust SDK](https://docs.rs/cloudevents-sdk), not a struct of our
//! own: it is a published spec with an implementation maintained by its authors,
//! so a consumer nobody here wrote can read the stream and any CloudEvents-aware
//! broker can route it. What this module adds is the two constructors a service
//! actually reaches for - one line instead of a builder chain - and a timestamp
//! the builder leaves unset. It sits in a crate about replication rather than in
//! `toolbox-core` because the SDK pulls `uuid` and `chrono`, which `toolbox-core`
//! refuses, and a crate of its own for two constructors would not earn the line.
//!
//! [`EventBus`] is the transport. It holds state across requests, so it is a
//! trait with adapters and declared capabilities, not a struct: a feature
//! needing replay fails at subscribe time on an adapter that cannot replay, in
//! development, rather than on a Tuesday in production. It depends on the
//! envelope the way `lock` depends on `deployment` for `Scope` - the payload is
//! always a [`CloudEvent`].

mod in_process;

use std::{pin::Pin, time::Duration};

use async_trait::async_trait;
use cloudevents::{EventBuilder, EventBuilderV10, event::Data};
use futures_core::Stream;
pub use in_process::InProcessEventBus;
use serde::Serialize;

/// A CloudEvents 1.0 event.
pub type CloudEvent = cloudevents::Event;

/// Why an event could not be built.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EventError {
    /// The payload could not be serialized.
    #[error("event payload: {0}")]
    Payload(#[from] serde_json::Error),
    /// The envelope was rejected, which means a required attribute was missing
    /// or malformed.
    #[error("event envelope: {0}")]
    Envelope(String),
}

/// An event carrying a JSON payload.
///
/// The id is generated and the time is set to now, which is what the builder
/// would have made you do by hand.
///
/// # Arguments
///
/// * `ty` - The CloudEvents `type`, in reverse-DNS form. It is what a
///   subscriber filters on, so it is part of the contract.
/// * `source` - The CloudEvents `source`: which service and instance produced
///   this.
/// * `data` - The payload, serialized as JSON.
///
/// # Errors
/// [`EventError`] when `data` cannot be serialized or the envelope is invalid.
pub fn event<T: Serialize>(
    ty: impl Into<String>,
    source: impl Into<String>,
    data: &T,
) -> Result<CloudEvent, EventError> {
    let payload = serde_json::to_value(data)?;
    EventBuilderV10::new()
        .id(uuid::Uuid::now_v7().to_string())
        .source(source.into())
        .ty(ty.into())
        .time(chrono::Utc::now())
        .data("application/json", payload)
        .build()
        .map_err(|e| EventError::Envelope(e.to_string()))
}

/// An event with no payload, for "this happened" with nothing to say about it.
///
/// # Arguments
///
/// * `ty` - The CloudEvents `type`, as for [`event`].
/// * `source` - The CloudEvents `source`, as for [`event`].
///
/// # Errors
/// [`EventError::Envelope`] when the envelope is invalid.
pub fn signal(ty: impl Into<String>, source: impl Into<String>) -> Result<CloudEvent, EventError> {
    EventBuilderV10::new()
        .id(uuid::Uuid::now_v7().to_string())
        .source(source.into())
        .ty(ty.into())
        .time(chrono::Utc::now())
        .build()
        .map_err(|e| EventError::Envelope(e.to_string()))
}

/// Read an event's payload as `T`.
///
/// # Arguments
///
/// * `event` - The event to read. It carries no payload at all for a
///   [`signal`], which is an error here rather than a default value.
///
/// # Errors
/// [`EventError::Payload`] when the event has no payload or it does not match
/// `T`.
pub fn payload<T: serde::de::DeserializeOwned>(event: &CloudEvent) -> Result<T, EventError> {
    let value = match event.data() {
        Some(Data::Json(value)) => value.clone(),
        Some(Data::String(text)) => serde_json::from_str(text)?,
        Some(Data::Binary(bytes)) => serde_json::from_slice(bytes)?,
        None => serde_json::Value::Null,
    };
    Ok(serde_json::from_value(value)?)
}

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
pub struct EventBusCapabilities {
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

impl EventBusCapabilities {
    /// Reject a [`StartPosition`] this adapter cannot serve, so the failure
    /// lands at subscribe time rather than as a silently short stream.
    ///
    /// # Arguments
    ///
    /// * `from` - Where the subscriber asked to start.
    /// * `adapter` - The adapter name, for the error.
    ///
    /// # Errors
    /// [`EventBusError::Unsupported`] when `from` is anything but the tail and
    /// this adapter has no `replay`.
    pub fn check_start(
        &self,
        from: &StartPosition,
        adapter: &'static str,
    ) -> Result<(), EventBusError> {
        match from {
            StartPosition::Now => Ok(()),
            StartPosition::Earliest | StartPosition::Cursor(_) if self.replay.is_some() => Ok(()),
            StartPosition::Earliest | StartPosition::Cursor(_) => Err(EventBusError::Unsupported {
                needed: MissingCapability::Replay,
                adapter,
            }),
        }
    }

    /// Reject a payload larger than [`EventBusCapabilities::max_payload`], for an
    /// adapter to call in `publish` before it hands the event to its transport.
    ///
    /// # Arguments
    ///
    /// * `size` - The serialized payload size in bytes.
    ///
    /// # Errors
    /// [`EventBusError::TooLarge`] when `size` is over the limit.
    pub fn check_payload(&self, size: usize) -> Result<(), EventBusError> {
        if size > self.max_payload {
            return Err(EventBusError::TooLarge {
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
    /// [`EventBusError::Unsupported`] with [`MissingCapability::Durability`] when
    /// this adapter is not durable.
    pub fn require_durable(&self, adapter: &'static str) -> Result<(), EventBusError> {
        if self.durable {
            return Ok(());
        }
        Err(EventBusError::Unsupported {
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
pub enum EventBusError {
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
    fn capabilities(&self) -> EventBusCapabilities;

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
    /// [`EventBusError`] when the payload is too large or the transport fails.
    async fn publish(&self, topic: &Topic, event: CloudEvent) -> Result<(), EventBusError>;

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
    /// [`EventBusError::Unsupported`] when `from` needs a capability this adapter
    /// does not have.
    async fn subscribe(
        &self,
        topic: &Topic,
        from: StartPosition,
    ) -> Result<EventStream, EventBusError>;
}
