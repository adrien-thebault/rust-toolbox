use chrono::DateTime;
use prost_types::{Timestamp, TimestampError};
use toolbox_grpc::proto::{datetime_to_timestamp, timestamp_to_datetime};

#[test]
fn timestamp_round_trips_a_utc_datetime() {
    let datetime = DateTime::parse_from_rfc3339("2026-09-27T12:34:56.123456789+02:00")
        .unwrap()
        .to_utc();

    let timestamp = datetime_to_timestamp(datetime);

    assert_eq!(timestamp_to_datetime(timestamp).unwrap(), datetime);
}

#[test]
fn timestamp_round_trips_an_instant_before_the_unix_epoch() {
    let datetime = DateTime::parse_from_rfc3339("1960-01-02T03:04:05.000000006Z")
        .unwrap()
        .to_utc();

    let timestamp = datetime_to_timestamp(datetime);

    assert_eq!(timestamp_to_datetime(timestamp).unwrap(), datetime);
}

#[test]
fn timestamp_rejects_invalid_nanoseconds() {
    for nanos in [-1, 1_000_000_000] {
        let result = timestamp_to_datetime(Timestamp { seconds: 0, nanos });

        assert_eq!(result, Err(TimestampError::InvalidDateTime));
    }
}

#[test]
fn timestamp_rejects_an_instant_outside_chronos_range() {
    let result = timestamp_to_datetime(Timestamp {
        seconds: i64::MAX,
        nanos: 0,
    });

    assert_eq!(result, Err(TimestampError::InvalidDateTime));
}
