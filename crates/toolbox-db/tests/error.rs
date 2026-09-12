use toolbox_db::DbError;
use toolbox_error::{ErrorKind, ServiceError};

/// The three variants a caller is allowed to see: everything else is an
/// internal fault whose text `ApiError` redacts.
#[test]
fn only_conflict_not_found_and_bad_sort_map_to_a_client_status() {
    assert_eq!(DbError::Conflict.kind(), ErrorKind::Conflict);
    assert_eq!(DbError::NotFound.kind(), ErrorKind::NotFound);
    assert_eq!(
        DbError::InvalidSortField {
            field: "x".to_owned(),
            allowed: "a, b".to_owned(),
        }
        .kind(),
        ErrorKind::InvalidArgument
    );
    assert_eq!(
        DbError::VersionOverflow.kind(),
        ErrorKind::Internal,
        "a caller never learns the version column overflowed"
    );
}

#[test]
fn every_variant_has_a_stable_code() {
    assert_eq!(DbError::Conflict.code(), "DB_CONFLICT");
    assert_eq!(DbError::NotFound.code(), "DB_NOT_FOUND");
    assert_eq!(
        DbError::InvalidSortField {
            field: "x".to_owned(),
            allowed: String::new(),
        }
        .code(),
        "INVALID_SORT_FIELD"
    );
    assert_eq!(DbError::Conflict.domain(), "db");
}

/// The internal-fault variants share one `ErrorKind` but keep distinct codes,
/// so a log line still says which layer failed even though the caller only
/// ever sees a 500.
#[test]
fn each_internal_fault_variant_keeps_its_own_code() {
    let query: DbError = diesel::result::Error::RollbackTransaction.into();
    let connection: DbError = diesel::ConnectionError::BadConnection("down".to_owned()).into();

    assert_eq!(query.code(), "DB_QUERY_FAILED");
    assert_eq!(connection.code(), "DB_CONNECTION_FAILED");
    assert_eq!(
        DbError::Migration("boom".to_owned()).code(),
        "DB_MIGRATION_FAILED"
    );
    assert_eq!(
        DbError::Interact("panicked".to_owned()).code(),
        "DB_TASK_FAILED"
    );
    assert_eq!(DbError::VersionOverflow.code(), "DB_VERSION_OVERFLOW");

    for e in [
        query,
        connection,
        DbError::Migration("x".to_owned()),
        DbError::VersionOverflow,
    ] {
        assert_eq!(e.kind(), ErrorKind::Internal);
        assert!(e.metadata().is_empty());
    }
}

/// r2d2 surfaces its own error type during migration setup; it must land on the
/// migration variant rather than a bare pool error.
#[test]
fn an_r2d2_error_maps_to_a_migration_failure() {
    let e: DbError =
        diesel::r2d2::Error::QueryError(diesel::result::Error::RollbackTransaction).into();
    assert_eq!(e.code(), "DB_MIGRATION_FAILED");
}

/// A rejected sort names what was asked for and the allowlist, so the caller
/// can fix the request rather than guess.
#[test]
fn an_invalid_sort_carries_the_field_and_the_allowlist_as_metadata() {
    let meta = DbError::InvalidSortField {
        field: "secret".to_owned(),
        allowed: "name, created_at".to_owned(),
    }
    .metadata();
    assert_eq!(meta.get("field").map(String::as_str), Some("secret"));
    assert_eq!(
        meta.get("allowed").map(String::as_str),
        Some("name, created_at")
    );
}

#[test]
fn a_conflict_reads_as_a_concurrent_write() {
    assert!(
        DbError::Conflict
            .to_string()
            .contains("modified concurrently")
    );
}
