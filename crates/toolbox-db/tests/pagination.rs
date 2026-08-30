use diesel::{prelude::*, sqlite::SqliteConnection};
use toolbox_core::{PageRequest, Sort};
use toolbox_db::{DbError, Paginate, pagination::validate};

use crate::fixtures::{TestEntity, seed, temp_db, test_entity};

#[tokio::test]
async fn a_page_carries_a_total_consistent_with_its_rows() {
    let (db, _dir) = temp_db();
    let page = db
        .query(|c: &mut SqliteConnection| {
            seed(c, 47);
            let req = PageRequest::paged(20, 10, Sort::unsorted()).unwrap();
            test_entity::table
                .select(TestEntity::as_select())
                .order(test_entity::id.asc())
                .paginate(&req)
                .load_page::<TestEntity, _>(c)
                .map_err(|_| diesel::result::Error::RollbackTransaction)
        })
        .await
        .unwrap();

    assert_eq!(page.len(), 10);
    assert_eq!(page.total(), 47);
    assert_eq!(page.page_number(), Some(2));
    assert_eq!(page.total_pages(), Some(5));
    assert_eq!(page.items()[0].id, 20);
}

#[tokio::test]
async fn the_last_page_is_short_and_reports_the_full_total() {
    let (db, _dir) = temp_db();
    let page = db
        .query(|c: &mut SqliteConnection| {
            seed(c, 25);
            let req = PageRequest::paged(20, 10, Sort::unsorted()).unwrap();
            test_entity::table
                .select(TestEntity::as_select())
                .order(test_entity::id.asc())
                .paginate(&req)
                .load_page::<TestEntity, _>(c)
                .map_err(|_| diesel::result::Error::RollbackTransaction)
        })
        .await
        .unwrap();

    assert_eq!(page.len(), 5);
    assert_eq!(page.total(), 25);
    assert!(!page.has_next());
}

#[tokio::test]
async fn an_empty_result_reports_a_total_of_zero() {
    let (db, _dir) = temp_db();
    let page = db
        .query(|c: &mut SqliteConnection| {
            let req = PageRequest::paged(0, 10, Sort::unsorted()).unwrap();
            test_entity::table
                .select(TestEntity::as_select())
                .paginate(&req)
                .load_page::<TestEntity, _>(c)
                .map_err(|_| diesel::result::Error::RollbackTransaction)
        })
        .await
        .unwrap();

    assert!(page.is_empty());
    assert_eq!(page.total(), 0);
    assert_eq!(page.total_pages(), Some(0));
}

/// The fix for "the moment you need a WHERE clause you lose pagination":
/// `paginate` composes onto any diesel query, not only generated ones.
#[tokio::test]
async fn pagination_composes_onto_a_hand_written_filter() {
    let (db, _dir) = temp_db();
    let page = db
        .query(|c: &mut SqliteConnection| {
            seed(c, 30);
            let req = PageRequest::paged(0, 5, Sort::unsorted()).unwrap();
            test_entity::table
                .filter(test_entity::rank.gt(19))
                .select(TestEntity::as_select())
                .order(test_entity::rank.asc())
                .paginate(&req)
                .load_page::<TestEntity, _>(c)
                .map_err(|_| diesel::result::Error::RollbackTransaction)
        })
        .await
        .unwrap();

    assert_eq!(page.len(), 5);
    assert_eq!(
        page.total(),
        10,
        "the total counts the filtered set, not the table"
    );
    assert_eq!(page.items()[0].rank, 20);
}

/// The window count comes from the same statement as the rows, so the two
/// cannot disagree - which a separate COUNT query can, and does.
#[tokio::test]
async fn an_unpaged_request_returns_everything_in_one_statement() {
    let (db, _dir) = temp_db();
    let page = db
        .query(|c: &mut SqliteConnection| {
            seed(c, 12);
            let req = PageRequest::unpaged(Sort::unsorted());
            test_entity::table
                .select(TestEntity::as_select())
                .paginate(&req)
                .load_page::<TestEntity, _>(c)
                .map_err(|_| diesel::result::Error::RollbackTransaction)
        })
        .await
        .unwrap();

    assert_eq!(page.len(), 12);
    assert_eq!(page.total(), 12);
    assert_eq!(page.page_number(), None);
}

#[tokio::test]
async fn an_offset_past_the_end_has_no_window_count_to_ride_back() {
    let (db, _dir) = temp_db();
    let page = db
        .query(|c: &mut SqliteConnection| {
            seed(c, 5);
            let req = PageRequest::paged(100, 10, Sort::unsorted()).unwrap();
            test_entity::table
                .select(TestEntity::as_select())
                .paginate(&req)
                .load_page::<TestEntity, _>(c)
                .map_err(|_| diesel::result::Error::RollbackTransaction)
        })
        .await
        .unwrap();

    assert!(page.is_empty());
    // No rows came back, so no window count did either. `load_page` is the
    // one-statement primitive and reports zero here; `Entity::page` reconciles
    // it against `Entity::count` (see the derive tests).
    assert_eq!(page.total(), 0);
}

const ALLOWED: &[&str] = &["id", "title", "created_at"];

#[test]
fn a_declared_sort_field_is_accepted() {
    assert!(validate(&Sort::parse("-created_at,title").unwrap(), ALLOWED).is_ok());
}

#[test]
fn an_empty_sort_is_accepted() {
    assert!(validate(&Sort::unsorted(), ALLOWED).is_ok());
}

/// An undeclared field must be rejected, never interpolated: this is the only
/// thing standing between a query parameter and SQL injection.
#[test]
fn an_undeclared_sort_field_is_rejected_and_names_the_allowlist() {
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
fn a_sort_injection_attempt_is_rejected_like_any_other_unknown_field() {
    let sort = Sort::parse("id; DROP TABLE users").unwrap();
    assert!(validate(&sort, ALLOWED).is_err());
}

#[test]
fn a_bad_sort_field_maps_to_a_client_mistake_not_a_server_fault() {
    use toolbox_core::{ErrorKind, ServiceError};
    let err = validate(&Sort::parse("nope").unwrap(), ALLOWED).unwrap_err();
    assert_eq!(err.kind(), ErrorKind::InvalidArgument);
    assert_eq!(err.code(), "INVALID_SORT_FIELD");
    assert_eq!(err.metadata()["field"], "nope");
}
