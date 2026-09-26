//! Protobuf types and conversion utilities.
//!
//! Timestamp conversions keep protobuf validation consistent across gRPC
//! services instead of repeating it at every wire boundary.

use chrono::{DateTime, Utc};
use prost_types::{Timestamp, TimestampError};

/// Types generated from `toolbox.v1`.
mod generated {
    #![allow(missing_docs, clippy::pedantic, clippy::all)]

    tonic::include_proto!("toolbox.v1");
}

pub use generated::*;

/// Converts a UTC datetime to its protobuf representation.
///
/// # Arguments
///
/// * `value` - UTC datetime to put on the protobuf wire.
#[must_use]
pub fn datetime_to_timestamp(value: DateTime<Utc>) -> Timestamp {
    Timestamp {
        seconds: value.timestamp(),
        nanos: value.timestamp_subsec_nanos().cast_signed(),
    }
}

/// Converts a protobuf timestamp to a validated UTC datetime.
///
/// # Arguments
///
/// * `value` - Protobuf timestamp received from the wire.
///
/// # Errors
///
/// Returns [`TimestampError::InvalidDateTime`] when nanoseconds are outside
/// their protobuf range or the instant is outside chrono's range.
pub fn timestamp_to_datetime(value: Timestamp) -> Result<DateTime<Utc>, TimestampError> {
    let nanos = u32::try_from(value.nanos)
        .ok()
        .filter(|nanos| *nanos < 1_000_000_000)
        .ok_or(TimestampError::InvalidDateTime)?;
    DateTime::from_timestamp(value.seconds, nanos).ok_or(TimestampError::InvalidDateTime)
}
