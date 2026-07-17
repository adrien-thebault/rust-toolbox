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
    #[cfg(any(feature = "mysql", feature = "postgresql"))]
    compile_error!("you can't use more than one database");

    /// database
    pub type Database = diesel::sqlite::Sqlite;

    /// database connection
    pub type DatabaseConnection = diesel::sqlite::SqliteConnection;
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
