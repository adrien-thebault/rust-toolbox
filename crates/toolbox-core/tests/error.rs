use std::collections::BTreeMap;

use toolbox_core::{ErrorKind, ServiceError};

#[derive(Debug, thiserror::Error)]
#[error("event {id} not found")]
struct NotFound {
    id: i64,
}

impl ServiceError for NotFound {
    fn code(&self) -> &'static str {
        "EVENT_NOT_FOUND"
    }
    fn domain(&self) -> &'static str {
        "events"
    }
    fn kind(&self) -> ErrorKind {
        ErrorKind::NotFound
    }
    fn metadata(&self) -> BTreeMap<String, String> {
        BTreeMap::from([("id".to_owned(), self.id.to_string())])
    }
}

#[test]
fn info_is_built_from_the_three_required_methods() {
    let info = NotFound { id: 7 }.info();
    assert_eq!(info.code, "EVENT_NOT_FOUND");
    assert_eq!(info.domain, "events");
    assert_eq!(info.metadata["id"], "7");
}

#[test]
fn metadata_defaults_to_empty() {
    #[derive(Debug, thiserror::Error)]
    #[error("boom")]
    struct Bare;
    impl ServiceError for Bare {
        fn code(&self) -> &'static str {
            "BOOM"
        }
        fn domain(&self) -> &'static str {
            "test"
        }
        fn kind(&self) -> ErrorKind {
            ErrorKind::Internal
        }
    }
    assert!(Bare.info().metadata.is_empty());
}

#[test]
fn error_info_serializes_metadata_in_deterministic_order() {
    let info = toolbox_core::ErrorInfo::new("X", "d")
        .with("b", "2")
        .with("a", "1");
    let json = serde_json::to_string(&info).unwrap();
    assert!(json.contains(r#""metadata":{"a":"1","b":"2"}"#), "{json}");
}

#[test]
fn only_transient_kinds_are_retryable() {
    assert!(ErrorKind::Unavailable.is_retryable());
    assert!(ErrorKind::Timeout.is_retryable());
    assert!(ErrorKind::ResourceExhausted.is_retryable());
    assert!(!ErrorKind::InvalidArgument.is_retryable());
    assert!(!ErrorKind::Internal.is_retryable());
}
