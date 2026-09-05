use diesel::{prelude::*, sqlite::SqliteConnection};

use crate::fixtures::temp_db;

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
