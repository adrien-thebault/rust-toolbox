//! The one test table, defined once.
//!
//! It used to be copy-pasted into four test files, which is how three of them
//! ended up subtly different.

use diesel::{connection::SimpleConnection, prelude::*, sqlite::SqliteConnection};
use toolbox_db::Db;

diesel::table! {
    test_entity (id) {
        id -> Integer,
        title -> Text,
        rank -> Integer,
        deleted_at -> Nullable<Timestamp>,
        version -> Integer,
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Queryable, Selectable, Insertable)]
#[diesel(table_name = test_entity)]
#[diesel(check_for_backend(diesel::sqlite::Sqlite))]
pub struct TestEntity {
    pub id: i32,
    pub title: String,
    pub rank: i32,
    pub deleted_at: Option<chrono::NaiveDateTime>,
    pub version: i32,
}

impl TestEntity {
    pub fn new(id: i32, title: &str, rank: i32) -> Self {
        Self {
            id,
            title: title.to_owned(),
            rank,
            deleted_at: None,
            version: 0,
        }
    }
}

pub const DDL: &str = "CREATE TABLE IF NOT EXISTS test_entity (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    rank INTEGER NOT NULL,
    deleted_at TIMESTAMP,
    version INTEGER NOT NULL DEFAULT 0
);";

/// A private, migrated, throwaway database.
///
/// Each call gets its own file so tests never share state; the file goes away
/// with the returned guard.
pub fn temp_db() -> (Db<SqliteConnection>, tempfile::TempDir) {
    let dir = tempfile::tempdir().expect("temp dir");
    let path = dir.path().join("test.sqlite3");
    let db = Db::<SqliteConnection>::builder(path.to_string_lossy().into_owned())
        .max_size(4)
        .sqlite_pragmas(toolbox_db::SqlitePragmas::default())
        .build()
        .expect("pool");

    let mut conn = db.blocking_conn().expect("connection");
    conn.batch_execute(DDL).expect("ddl");
    drop(conn);
    (db, dir)
}

/// Insert `n` rows, ranked 0..n.
pub fn seed(conn: &mut SqliteConnection, n: i32) {
    for i in 0..n {
        diesel::insert_into(test_entity::table)
            .values(TestEntity::new(i, &format!("row {i}"), i))
            .execute(conn)
            .expect("insert");
    }
}
