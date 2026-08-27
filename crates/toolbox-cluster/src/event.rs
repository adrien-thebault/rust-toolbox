//! The event envelope.
//!
//! CloudEvents 1.0, from the [official Rust SDK](https://docs.rs/cloudevents-sdk),
//! rather than a struct of our own. The envelope is a published specification
//! with an implementation already maintained by the people who wrote it, so a
//! consumer nobody here wrote can read the stream and any CloudEvents-aware
//! broker can route it.
//!
//! What this module adds is the two constructors a service actually reaches
//! for, so the common case is one line instead of a builder chain - and a
//! timestamp, which the builder leaves unset and an event stream needs.

use cloudevents::{EventBuilder, EventBuilderV10, event::Data};
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
/// * `type_` - The CloudEvents `type`, in reverse-DNS form. It is what a
///   subscriber filters on, so it is part of the contract.
/// * `source` - The CloudEvents `source`: which service and instance produced
///   this.
/// * `data` - The payload, serialized as JSON.
///
/// # Errors
/// [`EventError`] when `data` cannot be serialized or the envelope is invalid.
pub fn event<T: Serialize>(
    type_: impl Into<String>,
    source: impl Into<String>,
    data: &T,
) -> Result<CloudEvent, EventError> {
    let payload = serde_json::to_value(data)?;
    EventBuilderV10::new()
        .id(uuid::Uuid::now_v7().to_string())
        .source(source.into())
        .ty(type_.into())
        .time(chrono::Utc::now())
        .data("application/json", payload)
        .build()
        .map_err(|e| EventError::Envelope(e.to_string()))
}

/// An event with no payload, for "this happened" with nothing to say about it.
///
/// # Arguments
///
/// * `type_` - The CloudEvents `type`, as for [`event`].
/// * `source` - The CloudEvents `source`, as for [`event`].
///
/// # Errors
/// [`EventError::Envelope`] when the envelope is invalid.
pub fn signal(
    type_: impl Into<String>,
    source: impl Into<String>,
) -> Result<CloudEvent, EventError> {
    EventBuilderV10::new()
        .id(uuid::Uuid::now_v7().to_string())
        .source(source.into())
        .ty(type_.into())
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
