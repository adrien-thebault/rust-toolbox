mod forwarded_principal;
mod jwt;
mod password;
mod proxy_header;

use toolbox_auth::constant_time_eq;

/// The comparison every secret check rests on: it must not short-circuit on a
/// length difference or the first mismatching byte.
#[test]
fn constant_time_eq_matches_only_identical_byte_strings() {
    assert!(constant_time_eq(b"s3cr3t", b"s3cr3t"));
    assert!(constant_time_eq(b"", b""));

    assert!(!constant_time_eq(b"s3cr3t", b"s3cr3T"));
    assert!(!constant_time_eq(b"s3cr3t", b"S3cr3t"));
    assert!(!constant_time_eq(b"s3cr3t", b"s3cr3tt"));
    assert!(!constant_time_eq(b"s3cr3tt", b"s3cr3t"));
}
