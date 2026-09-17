//! Throwaway databases.

use std::{path::PathBuf, time::Duration};

use diesel::SqliteConnection;
use toolbox_db::{Db, DbPooledConn, EmbeddedMigrations, SqlitePragmas};

/// A database that deletes itself.
///
/// Holding the guard is what keeps the file alive, so bind it:
/// `let (db, _guard) = temp_db();`
#[derive(Debug)]
pub struct TempDb {
    /// The temp directory holding the database file; deleted on drop.
    dir: tempfile::TempDir,
}

impl TempDb {
    /// The database file's path.
    #[must_use]
    pub fn path(&self) -> PathBuf {
        self.dir.path().join("test.sqlite3")
    }
}

/// A private SQLite database and a pool over it.
///
/// Each call gets its own file, so tests never share state and can run in
/// parallel. The file goes away with the returned guard.
///
/// # Panics
/// When a temporary directory cannot be made or the pool cannot be built,
/// which in a test is a setup failure worth failing loudly on.
#[must_use]
pub fn temp_db() -> (Db<SqliteConnection>, TempDb) {
    let dir = tempfile::tempdir().expect("a temporary directory for the test database");
    let guard = TempDb { dir };
    let db = Db::<SqliteConnection>::builder(guard.path().to_string_lossy().into_owned())
        .max_size(4)
        .connect_timeout(Duration::from_secs(2))
        .sqlite_pragmas(SqlitePragmas::default())
        .build()
        .expect("a pool over the test database");
    (db, guard)
}

/// A [`temp_db`] with `migrations` already applied.
///
/// The `db()` helper at the top of every domain's `tests/common.rs`. Bind the
/// guard: `let (db, _guard) = migrated_db(MIGRATIONS).await;`.
///
/// # Panics
/// When the pool cannot be built or a migration fails - a setup failure worth
/// failing loudly on.
pub async fn migrated_db(migrations: EmbeddedMigrations) -> (Db<SqliteConnection>, TempDb) {
    let (db, guard) = temp_db();
    db.migrate(migrations).await.expect("run migrations");
    (db, guard)
}

/// A [`migrated_db`] plus one connection checked out of it, held for the whole
/// test so every write is seen by every later read.
///
/// The `conn()` helper for sync entity/repository tests. Bind the guard.
///
/// # Panics
/// When the pool cannot be built, a migration fails, or no connection is free.
pub async fn migrated_conn(
    migrations: EmbeddedMigrations,
) -> (DbPooledConn<SqliteConnection>, TempDb) {
    let (db, guard) = migrated_db(migrations).await;
    (db.blocking_conn().expect("check out a connection"), guard)
}
