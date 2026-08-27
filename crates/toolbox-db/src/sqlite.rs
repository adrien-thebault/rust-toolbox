//! SQLite connection pragmas.
//!
//! SQLite's default `busy_timeout` is 0, so the second connection to want a
//! write fails immediately rather than waiting. That is a trap the pool
//! guarantees you will hit, and it is why the value here is not optional.

use diesel::{
    connection::Connection,
    r2d2::{CustomizeConnection, Error as R2d2Error},
};

/// Pragmas applied to every pooled connection.
///
/// WAL permits many readers but exactly one writer, so a pool larger than one
/// plus concurrent writes produces `SQLITE_BUSY`. `busy_timeout` turns that
/// from an error into a wait. A pool of 4-8 is fine for reads; if write
/// contention shows up, that is the signal to move to PostgreSQL rather than to
/// raise the pool size.
#[derive(Debug, Clone, Copy)]
pub struct SqlitePragmas {
    /// How long a connection retries a locked database before failing.
    pub busy_timeout_ms: u32,
    /// Whether to enforce `REFERENCES` constraints. SQLite does not, per
    /// connection, unless asked.
    pub foreign_keys: bool,
    /// Whether to use write-ahead logging, which lets readers proceed during a
    /// write.
    pub wal: bool,
}

impl Default for SqlitePragmas {
    fn default() -> Self {
        Self {
            busy_timeout_ms: 5_000,
            foreign_keys: true,
            wal: true,
        }
    }
}

impl SqlitePragmas {
    /// Override the busy timeout.
    ///
    /// # Arguments
    ///
    /// * `ms` - How long a blocked writer waits before failing. Zero is
    ///   SQLite's default, and it is what turns a queued write into
    ///   `SQLITE_BUSY`.
    #[must_use]
    pub fn busy_timeout_ms(mut self, ms: u32) -> Self {
        self.busy_timeout_ms = ms;
        self
    }

    /// Enable or disable foreign-key enforcement.
    ///
    /// # Arguments
    ///
    /// * `enabled` - Whether to enforce foreign keys. SQLite leaves them off by
    ///   default, per connection.
    #[must_use]
    pub fn foreign_keys(mut self, enabled: bool) -> Self {
        self.foreign_keys = enabled;
        self
    }

    /// Enable or disable write-ahead logging.
    ///
    /// # Arguments
    ///
    /// * `enabled` - Whether to use write-ahead logging: many readers, exactly
    ///   one writer.
    #[must_use]
    pub fn wal(mut self, enabled: bool) -> Self {
        self.wal = enabled;
        self
    }
}

/// Applied to every connection the pool hands out.
///
/// Generic over the connection type because this crate has no backend feature.
/// Installing it on a pool that is not SQLite makes connection acquisition
/// fail, which is why `DbBuilder::sqlite_pragmas` is opt-in and named for the
/// backend it belongs to.
impl<C> CustomizeConnection<C, R2d2Error> for SqlitePragmas
where
    C: Connection + 'static,
{
    fn on_acquire(&self, conn: &mut C) -> Result<(), R2d2Error> {
        let mut sql = format!("PRAGMA busy_timeout = {};", self.busy_timeout_ms);
        if self.wal {
            sql.push_str(" PRAGMA journal_mode = WAL;");
        }
        if self.foreign_keys {
            sql.push_str(" PRAGMA foreign_keys = ON;");
        }
        conn.batch_execute(&sql).map_err(R2d2Error::QueryError)
    }
}
