use std::collections::BTreeMap;

use toolbox_core::{ErrorKind, PROBLEM_JSON, Problem, ServiceError};

#[derive(Debug, thiserror::Error)]
#[error("the database exploded: connection refused to 10.0.0.4:5432")]
struct Internal;

impl ServiceError for Internal {
    fn code(&self) -> &'static str {
        "DB_UNAVAILABLE"
    }
    fn domain(&self) -> &'static str {
        "events"
    }
    fn kind(&self) -> ErrorKind {
        ErrorKind::Internal
    }
    fn metadata(&self) -> BTreeMap<String, String> {
        BTreeMap::from([("host".to_owned(), "10.0.0.4".to_owned())])
    }
}

#[test]
fn media_type_is_the_rfc_9457_one() {
    assert_eq!(PROBLEM_JSON, "application/problem+json");
}

/// The wire format is the contract, so assert the exact JSON rather than the
/// Rust type: it is the assertion that would have caught the content-type bug.
#[test]
fn minimal_problem_omits_every_absent_member() {
    let json = serde_json::to_string(&Problem::new(404, "Not Found")).unwrap();
    assert_eq!(
        json,
        r#"{"type":"about:blank","title":"Not Found","status":404}"#
    );
}

#[test]
fn full_problem_uses_the_registered_member_names() {
    let p = Problem::new(409, "Conflict")
        .with_type("https://example.test/conflict")
        .with_detail("version mismatch")
        .with_request_id("abc123")
        .with_metadata("expected", "4");
    let v: serde_json::Value = serde_json::to_value(&p).unwrap();
    assert_eq!(v["type"], "https://example.test/conflict");
    assert_eq!(v["title"], "Conflict");
    assert_eq!(v["status"], 409);
    assert_eq!(v["detail"], "version mismatch");
    assert_eq!(v["request_id"], "abc123");
    assert_eq!(v["metadata"]["expected"], "4");
    assert!(v.get("instance").is_none());
}

#[test]
fn from_service_error_carries_code_domain_and_metadata() {
    let p = Problem::from_service_error(&Internal, 500);
    assert_eq!(p.code.as_deref(), Some("DB_UNAVAILABLE"));
    assert_eq!(p.domain.as_deref(), Some("events"));
    assert_eq!(p.title, "Internal Server Error");
    assert!(p.detail.is_some());
    assert_eq!(p.metadata["host"], "10.0.0.4");
}

#[test]
fn redact_removes_everything_a_5xx_must_not_disclose() {
    let mut p = Problem::from_service_error(&Internal, 500);
    p.redact();
    let json = serde_json::to_string(&p).unwrap();
    assert!(!json.contains("10.0.0.4"), "{json}");
    assert!(!json.contains("exploded"), "{json}");
    assert!(
        json.contains("DB_UNAVAILABLE"),
        "the stable code stays: {json}"
    );
}

#[test]
fn problem_round_trips() {
    let p = Problem::new(400, "Invalid Argument")
        .with_detail("bad")
        .with_metadata("f", "v");
    let back: Problem = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
    assert_eq!(p, back);
}
