use thiserror::Error;

// the `diesel` feature alone doesn't pick a backend, and everything below
// needs one - fail with a message instead of a wall of unresolved-type errors.
#[cfg(not(any(feature = "postgresql", feature = "mysql", feature = "sqlite")))]
compile_error!(
    "the `diesel` feature needs a backend: enable exactly one of `sqlite`, `postgresql` or `mysql`"
);

/// PostgreSQL
#[cfg(feature = "postgresql")]
mod postgresql {
    #[cfg(any(feature = "mysql", feature = "sqlite"))]
    compile_error!("you can't use more than one database");

    /// database
    pub type Database = diesel::pg::Pg;

    /// database connection
    pub type DatabaseConnection = diesel::pg::PgConnection;
}

#[cfg(feature = "postgresql")]
pub use postgresql::*;

/// MySQL / MariaDB
#[cfg(feature = "mysql")]
mod mysql {
    #[cfg(any(feature = "postgresql", feature = "sqlite"))]
    compile_error!("you can't use more than one database");

    /// database
    pub type Database = diesel::mysql::Mysql;

    /// database connection
    pub type DatabaseConnection = diesel::mysql::MysqlConnection;
}

#[cfg(feature = "mysql")]
pub use mysql::*;

/// SQLite
#[cfg(feature = "sqlite")]
mod sqlite {
    use diesel::RunQueryDsl;
    use diesel::r2d2::{CustomizeConnection, Error};

    #[cfg(any(feature = "mysql", feature = "postgresql"))]
    compile_error!("you can't use more than one database");

    /// database
    pub type Database = diesel::sqlite::Sqlite;

    /// database connection
    pub type DatabaseConnection = diesel::sqlite::SqliteConnection;

    /// customizes every pooled SQLite connection with the pragmas that
    /// start mattering once more than one connection touches the same file
    /// - which an r2d2 pool guarantees will eventually happen:
    /// - `busy_timeout`: SQLite's own default is 0ms, so a connection that
    ///   finds the file locked by another connection fails immediately with
    ///   "database is locked" instead of waiting. Setting this makes a
    ///   blocked connection retry internally for up to `busy_timeout_ms`
    ///   before giving up - a lock held for a normal single write/txn
    ///   duration is then waited out instead of surfacing as an error.
    /// - `foreign_keys`: SQLite doesn't enforce `REFERENCES` constraints
    ///   unless this is set on each connection; left off by default since
    ///   not every schema has them (enable per service via `.foreign_keys(true)`).
    ///
    /// ```ignore
    /// DatabasePool::builder()
    ///     .connection_customizer(Box::new(SqlitePragmas::default().foreign_keys(true)))
    ///     .build(DatabaseManager::new(database_url))?;
    /// ```
    #[derive(Debug, Clone, Copy)]
    pub struct SqlitePragmas {
        /// milliseconds a connection spends retrying a locked database
        /// before giving up with "database is locked"; defaults to 30_000
        pub busy_timeout_ms: u32,
        /// whether to run `PRAGMA foreign_keys = ON;` on every connection;
        /// defaults to `false`
        pub foreign_keys: bool,
    }

    impl Default for SqlitePragmas {
        fn default() -> Self {
            Self {
                busy_timeout_ms: 30_000,
                foreign_keys: false,
            }
        }
    }

    impl SqlitePragmas {
        /// overrides the default 30_000ms busy timeout
        pub fn busy_timeout_ms(mut self, ms: u32) -> Self {
            self.busy_timeout_ms = ms;
            self
        }

        /// enables `PRAGMA foreign_keys = ON;` on every connection
        pub fn foreign_keys(mut self, enabled: bool) -> Self {
            self.foreign_keys = enabled;
            self
        }
    }

    impl CustomizeConnection<DatabaseConnection, Error> for SqlitePragmas {
        fn on_acquire(&self, conn: &mut DatabaseConnection) -> Result<(), Error> {
            diesel::sql_query(format!("PRAGMA busy_timeout = {};", self.busy_timeout_ms))
                .execute(conn)
                .map_err(Error::QueryError)?;
            if self.foreign_keys {
                diesel::sql_query("PRAGMA foreign_keys = ON;")
                    .execute(conn)
                    .map_err(Error::QueryError)?;
            }
            Ok(())
        }
    }
}

#[cfg(feature = "sqlite")]
pub use sqlite::*;

/// database connection Manager
pub type DatabaseManager = diesel::r2d2::ConnectionManager<DatabaseConnection>;

/// database Pool
pub type DatabasePool = diesel::r2d2::Pool<DatabaseManager>;

/// database Pooled Connection
pub type DatabasePooledConnection = diesel::r2d2::PooledConnection<DatabaseManager>;

/// database result type
pub type DatabaseResult<T> = Result<T, DatabaseError>;

/// database errors
#[derive(Error, Debug)]
pub enum DatabaseError {
    /// connection error
    #[error("database connection error -> {0}")]
    Connection(#[from] diesel::ConnectionError),

    /// pool error
    #[error("database pool error -> {0}")]
    Pool(#[from] diesel::r2d2::PoolError),

    /// migration error. Deliberately no `From<Box<dyn Error>>` for this
    /// variant - a blanket conversion would turn *any* boxed error reached
    /// via `?` into a "migration error"; construct it explicitly at the
    /// call site that actually runs migrations.
    #[error("database migration error -> {0}")]
    Migration(Box<dyn std::error::Error + Send + Sync>),

    /// query error
    #[error("database query error -> {0}")]
    Query(#[from] diesel::result::Error),
}
