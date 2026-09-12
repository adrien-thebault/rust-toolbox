//! The CloudEvents envelope and the contract for moving it between replicas.
//!
//! The envelope is CloudEvents 1.0 from the
//! [official Rust SDK](https://docs.rs/cloudevents-sdk), not a struct of our
//! own: it is a published spec with an implementation maintained by its authors,
//! so a consumer nobody here wrote can read the stream and any CloudEvents-aware
//! broker can route it. What this module adds is the two constructors a service
//! actually reaches for - one line instead of a builder chain - and a timestamp
//! the builder leaves unset. It sits in a crate about replication rather than
//! in one of the toolbox's dependency-free vocabulary crates, because the SDK
//! pulls `uuid` and `chrono`, which those refuse, and a crate of its own for
//! two constructors would not earn the line.
//!
//! [`EventBus`] is the transport. It holds state across requests, so it is a
//! trait with a local adapter and at least one shared adapter; the payload is
//! always a [`CloudEvent`].

mod in_memory;

use std::pin::Pin;

use async_trait::async_trait;
use cloudevents::{EventBuilder, EventBuilderV10, event::Data};
use futures_core::Stream;
pub use in_memory::InMemoryEventBus;
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

/// Why a bus operation failed.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum EventBusError {
    /// The adapter's transport failed.
    #[error("event bus transport: {0}")]
    Transport(String),
}

/// A stream of events from a subscription.
pub type EventStream = Pin<Box<dyn Stream<Item = CloudEvent> + Send>>;

/// Publish and subscribe to events.
///
/// Every adapter starts a subscription from the tail - the only shape this
/// workspace's one consumer, `toolbox-web`'s SSE reconnect story, has ever
/// needed. It resumes a gap by re-querying the domain for what it missed, then
/// subscribing live; a bus with replay is a future adapter's addition, not a
/// contract every adapter carries today. Delivery and durability genuinely
/// differ per adapter and belong in its own doc comment, not a negotiated
/// capability: [`InMemoryEventBus`] is at-most-once and drops on lag, and a
/// shared adapter would document its own guarantee the same way.
#[async_trait]
pub trait EventBus: Send + Sync {
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
    /// [`EventBusError`] when the transport fails.
    async fn publish(&self, topic: &Topic, event: CloudEvent) -> Result<(), EventBusError>;

    /// Subscribe to a topic, from now on.
    ///
    /// # Arguments
    ///
    /// * `topic` - What to subscribe to.
    ///
    /// # Errors
    /// [`EventBusError`] when the transport fails.
    async fn subscribe(&self, topic: &Topic) -> Result<EventStream, EventBusError>;
}
