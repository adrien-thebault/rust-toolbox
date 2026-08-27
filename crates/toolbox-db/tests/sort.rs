use toolbox_core::Sort;
use toolbox_db::{DbError, sort::validate};

const ALLOWED: &[&str] = &["id", "title", "created_at"];

#[test]
fn a_declared_field_is_accepted() {
    assert!(validate(&Sort::parse("-created_at,title").unwrap(), ALLOWED).is_ok());
}

#[test]
fn an_empty_sort_is_accepted() {
    assert!(validate(&Sort::unsorted(), ALLOWED).is_ok());
}

/// An undeclared field must be rejected, never interpolated: this is the only
/// thing standing between a query parameter and SQL injection.
#[test]
fn an_undeclared_field_is_rejected_and_names_the_allowlist() {
    let err = validate(&Sort::parse("password").unwrap(), ALLOWED).unwrap_err();
    match err {
        DbError::InvalidSortField { field, allowed } => {
            assert_eq!(field, "password");
            assert_eq!(allowed, "id, title, created_at");
        }
        other => panic!("expected InvalidSortField, got {other:?}"),
    }
}

#[test]
fn an_injection_attempt_is_rejected_like_any_other_unknown_field() {
    let sort = Sort::parse("id; DROP TABLE users").unwrap();
    assert!(validate(&sort, ALLOWED).is_err());
}

#[test]
fn the_error_maps_to_a_client_mistake_not_a_server_fault() {
    use toolbox_core::{ErrorKind, ServiceError};
    let err = validate(&Sort::parse("nope").unwrap(), ALLOWED).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidArgument);
    assert_eq!(err.code(), "INVALID_SORT_FIELD");
    assert_eq!(err.metadata()["field"], "nope");
}
