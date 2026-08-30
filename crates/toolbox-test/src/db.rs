//! Throwaway databases.

use std::path::PathBuf;

use diesel::r2d2::R2D2Connection;
use toolbox_db::{Db, SqlitePragmas};

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
pub fn temp_db<C: R2D2Connection + 'static>() -> (Db<C>, TempDb) {
    let dir = tempfile::tempdir().expect("a temporary directory for the test database");
    let guard = TempDb { dir };
    let db = Db::<C>::builder(guard.path().to_string_lossy().into_owned())
        .max_size(4)
        .connect_timeout(std::time::Duration::from_secs(2))
        .sqlite_pragmas(SqlitePragmas::default())
        .build()
        .expect("a pool over the test database");
    (db, guard)
}
