use std::collections::BTreeMap;

use toolbox_core::{ErrorKind, ServiceError};

#[derive(Debug, thiserror::Error)]
#[error("inner boom")]
struct Inner;

impl ServiceError for Inner {
    fn code(&self) -> &'static str {
        "INNER"
    }
    fn domain(&self) -> &'static str {
        "inner.domain"
    }
    fn kind(&self) -> ErrorKind {
        ErrorKind::Unavailable
    }
    fn metadata(&self) -> BTreeMap<String, String> {
        BTreeMap::from([("k".to_owned(), "v".to_owned())])
    }
}

#[derive(Debug, thiserror::Error, ServiceError)]
#[service_error(domain = "test.svc")]
enum E {
    #[error("not found: {0}")]
    #[service_error(code = "NOT_FOUND", kind = NotFound, meta(id = 0))]
    NotFound(i32),

    #[error("bad {reason}")]
    #[service_error(code = "BAD", kind = InvalidArgument)]
    Bad { reason: String },

    #[error(transparent)]
    #[service_error(transparent)]
    Inner(#[from] Inner),
}

fn main() {
    let e = E::NotFound(7);
    assert_eq!(e.code(), "NOT_FOUND");
    assert_eq!(e.kind(), ErrorKind::NotFound);
    assert_eq!(e.domain(), "test.svc");
    assert_eq!(e.metadata().get("id").map(String::as_str), Some("7"));

    let e = E::Bad {
        reason: "x".to_owned(),
    };
    assert_eq!(e.code(), "BAD");
    assert_eq!(e.kind(), ErrorKind::InvalidArgument);
    assert!(e.metadata().is_empty());

    // `transparent` delegates code/kind/metadata to the inner error, but
    // `domain` is always the enum's own.
    let e = E::from(Inner);
    assert_eq!(e.code(), "INNER");
    assert_eq!(e.kind(), ErrorKind::Unavailable);
    assert_eq!(e.domain(), "test.svc");
    assert_eq!(e.metadata().get("k").map(String::as_str), Some("v"));
}
