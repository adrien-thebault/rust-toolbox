//! shared fixtures for the `diesel_tools` integration tests: two tables
//! (plain-id `widgets`, autoincrement `gadgets`) with their entities and
//! macro-generated repositories, plus a backend-generic [`setup`].

use diesel::prelude::*;
use rust_toolbox::diesel_tools::{DatabaseManager, DatabasePool, DatabasePooledConnection};
use rust_toolbox::impl_repository;
use std::sync::{Mutex, MutexGuard, PoisonError};

diesel::table! {
    widgets (id) {
        id -> Integer,
        name -> Text,
        rank -> Integer,
    }
}

#[derive(
    Clone, Queryable, Selectable, Identifiable, Insertable, AsChangeset, PartialEq, Eq, Debug,
)]
#[diesel(table_name = widgets)]
pub struct Widget {
    pub id: i32,
    pub name: String,
    pub rank: i32,
}

pub struct WidgetRepository;

impl_repository!(WidgetRepository {
    Schema = crate::common::widgets,
    Entity = Widget,
    Id = (i32, id),
    SortColumns = { name, rank },
});

diesel::table! {
    gadgets (id) {
        id -> Integer,
        name -> Text,
    }
}

#[derive(Clone, Queryable, Selectable, Insertable, AsChangeset, PartialEq, Eq, Debug)]
#[diesel(table_name = gadgets)]
pub struct Gadget {
    #[diesel(deserialize_as = i32)]
    pub id: Option<i32>,
    pub name: String,
}

pub struct GadgetRepository;

impl_repository!(GadgetRepository {
    Schema = crate::common::gadgets,
    Entity = Gadget,
    Id = (i32, id, autoincrement),
});

pub fn widget(id: i32, name: &str, rank: i32) -> Widget {
    Widget {
        id,
        name: name.to_string(),
        rank,
    }
}

// per-backend DDL. The CHECK (rank >= 0) constraint exists so tests can
// force a mid-batch failure that isn't a duplicated key - MySQL's upsert
// (ON DUPLICATE KEY UPDATE) absorbs *any* unique-key conflict, so a UNIQUE
// column wouldn't fail portably. `name` is VARCHAR on MySQL because bare
// TEXT columns can't be indexed there; TEXT elsewhere.
#[cfg(feature = "sqlite")]
const DDL: [&str; 2] = [
    "CREATE TABLE widgets (id INTEGER PRIMARY KEY NOT NULL, name TEXT NOT NULL, \
     rank INTEGER NOT NULL CHECK (rank >= 0))",
    "CREATE TABLE gadgets (id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL)",
];

#[cfg(feature = "postgresql")]
const DDL: [&str; 2] = [
    "CREATE TABLE widgets (id INTEGER PRIMARY KEY, name TEXT NOT NULL, \
     rank INTEGER NOT NULL CHECK (rank >= 0))",
    "CREATE TABLE gadgets (id SERIAL PRIMARY KEY, name TEXT NOT NULL)",
];

#[cfg(feature = "mysql")]
const DDL: [&str; 2] = [
    "CREATE TABLE widgets (id INTEGER PRIMARY KEY, name VARCHAR(255) NOT NULL, \
     `rank` INTEGER NOT NULL, CHECK (`rank` >= 0))",
    "CREATE TABLE gadgets (id INTEGER NOT NULL AUTO_INCREMENT, name VARCHAR(255) NOT NULL, \
     PRIMARY KEY (id))",
];

/// where this backend's test database lives. SQLite always has one
/// (in-memory); PostgreSQL/MySQL need a real server, named by an env var so
/// CI (service containers) and local runs can provide their own.
#[cfg(feature = "sqlite")]
fn database_url() -> Option<String> {
    Some(":memory:".to_string())
}

#[cfg(feature = "postgresql")]
fn database_url() -> Option<String> {
    std::env::var("TOOLBOX_TEST_POSTGRES_URL").ok()
}

#[cfg(feature = "mysql")]
fn database_url() -> Option<String> {
    std::env::var("TOOLBOX_TEST_MYSQL_URL").ok()
}

/// a live connection to a freshly (re)created pair of test tables. Holds a
/// lock for the test's duration: SQLite gets a private `:memory:` database
/// per test, but PostgreSQL/MySQL tests share one server-side database and
/// its table names, so DB tests must not run concurrently.
pub struct TestDb {
    pub conn: DatabasePooledConnection,
    _lock: MutexGuard<'static, ()>,
}

static LOCK: Mutex<()> = Mutex::new(());

/// `None` (after an explanatory note on stderr) when the backend's env var
/// is unset - callers soft-skip with `let Some(mut db) = setup() else { return };`
/// so a plain local `cargo test --features postgresql` still passes without
/// a server.
pub fn setup() -> Option<TestDb> {
    let Some(url) = database_url() else {
        eprintln!(
            "skipping: no test database configured for this backend \
             (set TOOLBOX_TEST_POSTGRES_URL / TOOLBOX_TEST_MYSQL_URL)"
        );
        return None;
    };

    // a poisoned lock only means another DB test panicked - its tables get
    // dropped and recreated below anyway
    let lock = LOCK.lock().unwrap_or_else(PoisonError::into_inner);

    // max_size(1): every r2d2 connection to SQLite `:memory:` would
    // otherwise get its own independent database
    let pool: DatabasePool = DatabasePool::builder()
        .max_size(1)
        .build(DatabaseManager::new(url))
        .expect("building the test pool");
    let mut conn = pool.get().expect("checking out the test connection");

    for table in ["widgets", "gadgets"] {
        diesel::sql_query(format!("DROP TABLE IF EXISTS {table}"))
            .execute(&mut conn)
            .expect("dropping leftover test table");
    }
    for ddl in DDL {
        diesel::sql_query(ddl)
            .execute(&mut conn)
            .expect("creating test table");
    }

    Some(TestDb { conn, _lock: lock })
}
