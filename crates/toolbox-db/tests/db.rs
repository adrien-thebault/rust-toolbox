use diesel::{prelude::*, sqlite::SqliteConnection};
use toolbox_db::{DbError, DbResult};

use crate::fixtures::{TestEntity, seed, temp_db, test_entity};

#[tokio::test]
async fn run_returns_the_closures_own_error_type_unchanged() {
    #[derive(Debug, thiserror::Error)]
    enum MyError {
        #[error("domain")]
        Domain,
        #[error(transparent)]
        Db(#[from] DbError),
    }

    let (db, _dir) = temp_db();
    let err = db
        .run(|_c: &mut SqliteConnection| Err::<(), MyError>(MyError::Domain))
        .await;
    assert!(matches!(err, Err(MyError::Domain)));
}

#[tokio::test]
async fn run_hands_out_a_working_connection() {
    let (db, _dir) = temp_db();
    let n = db
        .query(|c: &mut SqliteConnection| {
            seed(c, 3);
            test_entity::table.count().get_result::<i64>(c)
        })
        .await
        .unwrap();
    assert_eq!(n, 3);
}

#[tokio::test]
async fn transaction_rolls_back_on_error() {
    let (db, _dir) = temp_db();
    let result: DbResult<()> = db
        .transaction(|c: &mut SqliteConnection| {
            seed(c, 2);
            Err(DbError::Conflict)
        })
        .await;
    assert!(matches!(result, Err(DbError::Conflict)));

    let n = db
        .query(|c: &mut SqliteConnection| test_entity::table.count().get_result::<i64>(c))
        .await
        .unwrap();
    assert_eq!(n, 0, "the failed transaction left nothing behind");
}

#[tokio::test]
async fn transaction_commits_on_success() {
    let (db, _dir) = temp_db();
    db.transaction(|c: &mut SqliteConnection| {
        seed(c, 2);
        Ok::<_, DbError>(())
    })
    .await
    .unwrap();

    let n = db
        .query(|c: &mut SqliteConnection| test_entity::table.count().get_result::<i64>(c))
        .await
        .unwrap();
    assert_eq!(n, 2);
}

#[tokio::test]
async fn a_panicking_closure_becomes_an_error_rather_than_killing_the_caller() {
    let (db, _dir) = temp_db();
    let result: DbResult<()> = db.run(|_c: &mut SqliteConnection| panic!("boom")).await;
    assert!(matches!(result, Err(DbError::Interact(_))), "{result:?}");
}

#[tokio::test]
async fn run_named_behaves_exactly_like_run() {
    let (db, _dir) = temp_db();
    let n = db
        .query_named("count_things", |c: &mut SqliteConnection| {
            seed(c, 5);
            test_entity::table.count().get_result::<i64>(c)
        })
        .await
        .unwrap();
    assert_eq!(n, 5);
}

#[tokio::test]
async fn clones_share_one_pool() {
    let (db, _dir) = temp_db();
    let other = db.clone();
    db.run(|c: &mut SqliteConnection| {
        seed(c, 1);
        Ok::<_, DbError>(())
    })
    .await
    .unwrap();
    let n = other
        .query(|c: &mut SqliteConnection| test_entity::table.count().get_result::<i64>(c))
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[test]
fn debug_never_prints_the_url_because_it_carries_the_password() {
    let (db, _dir) = temp_db();
    let rendered = format!("{db:?}");
    assert!(!rendered.contains("sqlite3"), "{rendered}");
}

/// The pragmas are what make a multi-connection pool usable at all: without a
/// busy timeout the second writer fails instead of waiting.
#[tokio::test]
async fn the_sqlite_pragmas_are_applied_to_every_connection() {
    let (db, _dir) = temp_db();
    let timeout = db
        .query(|c: &mut SqliteConnection| {
            diesel::sql_query("PRAGMA busy_timeout")
                .get_result::<BusyTimeout>(c)
                .map(|r| r.timeout)
        })
        .await
        .unwrap();
    assert_eq!(timeout, 5_000);
}

#[derive(QueryableByName)]
struct BusyTimeout {
    #[diesel(sql_type = diesel::sql_types::Integer, column_name = timeout)]
    timeout: i32,
}

#[tokio::test]
async fn foreign_keys_are_enforced_by_default() {
    let (db, _dir) = temp_db();
    let on = db
        .query(|c: &mut SqliteConnection| {
            diesel::sql_query("PRAGMA foreign_keys")
                .get_result::<ForeignKeys>(c)
                .map(|r| r.foreign_keys)
        })
        .await
        .unwrap();
    assert_eq!(on, 1);
}

#[derive(QueryableByName)]
struct ForeignKeys {
    #[diesel(sql_type = diesel::sql_types::Integer, column_name = foreign_keys)]
    foreign_keys: i32,
}

#[test]
fn blocking_conn_is_the_named_escape_hatch() {
    let (db, _dir) = temp_db();
    let mut conn = db.blocking_conn().unwrap();
    seed(&mut conn, 1);
    assert_eq!(
        test_entity::table
            .count()
            .get_result::<i64>(&mut conn)
            .unwrap(),
        1
    );
}

#[test]
fn an_unreachable_database_fails_at_build_time_not_at_first_query() {
    let err = toolbox_db::Db::<SqliteConnection>::builder("/nonexistent-dir/x/y.sqlite3")
        .connect_timeout(std::time::Duration::from_millis(200))
        .build();
    assert!(err.is_err());
}

#[allow(dead_code)]
fn entity_is_reexported(_: Option<&dyn Fn() -> TestEntity>) {}
