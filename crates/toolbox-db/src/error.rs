//! One error type for everything the database layer can fail at.

use std::collections::BTreeMap;

use toolbox_core::{ErrorKind, ServiceError};

/// A database failure.
///
/// One type rather than r2d2's, diesel's and tokio's, so a caller writes one
/// `From` impl instead of three.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum DbError {
    /// The pool could not hand out a connection within its timeout.
    #[error("database pool: {0}")]
    Pool(#[from] diesel::r2d2::PoolError),
    /// A connection could not be established.
    #[error("database connection: {0}")]
    Connection(#[from] diesel::ConnectionError),
    /// A query failed.
    #[error("database query: {0}")]
    Query(#[from] diesel::result::Error),
    /// A migration failed.
    #[error("database migration: {0}")]
    Migration(String),
    /// The blocking task carrying the query panicked or was cancelled.
    #[error("database task did not complete: {0}")]
    Interact(String),
    /// An optimistic-locking check matched no rows: someone else wrote first.
    #[error("the row was modified concurrently")]
    Conflict,
    /// The optimistic-locking version column has no room left to increment.
    #[error("the version column overflowed its integer type")]
    VersionOverflow,
    /// The row does not exist.
    #[error("no such row")]
    NotFound,
    /// A sort was requested on a field the entity does not declare sortable.
    ///
    /// Rejected rather than interpolated, which is what keeps a sort parameter
    /// from being an injection point.
    #[error("cannot sort by `{field}`; sortable fields are: {allowed}")]
    InvalidSortField {
        /// What the caller asked for.
        field: String,
        /// The declared allowlist, comma-separated.
        allowed: String,
    },
}

/// The result of a database operation.
pub type DbResult<T> = Result<T, DbError>;

impl ServiceError for DbError {
    fn code(&self) -> &'static str {
        match self {
            Self::Pool(_) => "DB_POOL_EXHAUSTED",
            Self::Connection(_) => "DB_CONNECTION_FAILED",
            Self::Query(_) => "DB_QUERY_FAILED",
            Self::Migration(_) => "DB_MIGRATION_FAILED",
            Self::Interact(_) => "DB_TASK_FAILED",
            Self::Conflict => "DB_CONFLICT",
            Self::VersionOverflow => "DB_VERSION_OVERFLOW",
            Self::NotFound => "DB_NOT_FOUND",
            Self::InvalidSortField { .. } => "INVALID_SORT_FIELD",
        }
    }

    fn domain(&self) -> &'static str {
        "db"
    }

    fn kind(&self) -> ErrorKind {
        match self {
            Self::Conflict => ErrorKind::Conflict,
            Self::NotFound => ErrorKind::NotFound,
            Self::InvalidSortField { .. } => ErrorKind::InvalidArgument,
            // Everything else is an internal fault, so its text never reaches
            // a caller: `ApiError` redacts 5xx detail.
            _ => ErrorKind::Internal,
        }
    }

    fn metadata(&self) -> BTreeMap<String, String> {
        match self {
            Self::InvalidSortField { field, allowed } => BTreeMap::from([
                ("field".to_owned(), field.clone()),
                ("allowed".to_owned(), allowed.clone()),
            ]),
            _ => BTreeMap::new(),
        }
    }
}

impl From<diesel::r2d2::Error> for DbError {
    fn from(e: diesel::r2d2::Error) -> Self {
        Self::Migration(e.to_string())
    }
}
