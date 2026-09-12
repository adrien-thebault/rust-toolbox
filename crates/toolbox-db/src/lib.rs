//! Diesel with the blocking path made unreachable from the async API.
//!
//! Every diesel call reachable from an `async fn` goes through [`Db::run`],
//! which runs it on a blocking thread. The one escape hatch is called
//! `blocking_conn` so it is visible in review.
//!
//! # No backend features
//!
//! This crate declares no `sqlite`, `postgres` or `mysql` feature. It is
//! generic over `C: R2D2Connection` and the consumer's own `diesel` dependency
//! selects the backend. Three things follow, and they are the reason the
//! design is this way:
//!
//! - `--all-features` and `cargo hack --feature-powerset` work, where mutually
//!   exclusive backend features made them impossible;
//! - one process can hold a PostgreSQL pool and a SQLite pool at once;
//! - a feature enabled anywhere in a workspace cannot silently change which
//!   backend another crate compiles against.
//!
//! The `clap` feature here adds the argument struct. It is not a backend
//! selector. `#[derive(Entity)]`'s timestamp autofill always uses `chrono`,
//! the one datetime library this workspace names - see `CLAUDE.md`.

pub mod args;
pub mod db;
pub mod entity;
pub mod error;
pub mod migrate;
pub mod pagination;
pub mod sqlite;

pub use db::{Db, DbBuilder, DbPool, DbPooledConn};
/// Re-exported so `#[derive(Entity)]` can name it without the consumer
/// declaring `diesel_migrations` itself. `MigrationHarness` is the bound
/// `Db::migrate` needs, re-exported so a helper that forwards to it (a test's
/// `migrated_db`) can repeat that bound.
pub use diesel_migrations::{EmbeddedMigrations, MigrationHarness, embed_migrations};
pub use entity::Entity;
pub use error::{DbError, DbResult};
pub use pagination::{Paginate, Paginated};
pub use sqlite::SqlitePragmas;
/// The derive. Shares its name with the [`Entity`] trait it implements, the
/// way `serde::Serialize` does: derives live in the macro namespace.
pub use toolbox_macros::Entity;

/// Name this crate's SQLite backend and connection types, once.
///
/// Every domain crate names its diesel backend and connection type once, in
/// `lib.rs`, so `#[entity(backend = crate::Backend)]` and every
/// `Db<crate::Connection>` refer to one place and swapping backends is a
/// one-line change. That block is the same two lines in every crate; this
/// macro is that block.
///
/// It expands in the consumer, which is where `diesel` with its `sqlite`
/// feature is declared - `toolbox-db` itself selects no backend (see the crate
/// docs), so it cannot name `diesel::sqlite::Sqlite` on its own.
///
/// ```ignore
/// toolbox_db::sqlite_backend!();
/// // expands to:
/// pub type Backend = ::diesel::sqlite::Sqlite;
/// pub type Connection = ::diesel::sqlite::SqliteConnection;
/// ```
#[macro_export]
macro_rules! sqlite_backend {
    () => {
        /// The database backend, named once for the whole crate.
        ///
        /// The one place a diesel backend type appears:
        /// `#[entity(backend = crate::Backend)]` and `check_for_backend`
        /// refer here, so swapping backends is this call plus the connection
        /// URL.
        pub type Backend = ::diesel::sqlite::Sqlite;

        /// The connection type, following from [`Backend`].
        pub type Connection = ::diesel::sqlite::SqliteConnection;
    };
}

/// Name this crate's PostgreSQL backend and connection types, once.
///
/// Invoke once at the top of a domain crate's `lib.rs`. Needs `diesel` (with
/// its `postgres` feature) in the consumer's dependencies. See
/// [`sqlite_backend!`] for the rationale.
#[macro_export]
macro_rules! postgres_backend {
    () => {
        /// The database backend, named once for the whole crate.
        ///
        /// The one place a diesel backend type appears:
        /// `#[entity(backend = crate::Backend)]` and `check_for_backend`
        /// refer here, so swapping backends is this call plus the connection
        /// URL.
        pub type Backend = ::diesel::pg::Pg;

        /// The connection type, following from [`Backend`].
        pub type Connection = ::diesel::pg::PgConnection;
    };
}

/// Name this crate's MySQL/MariaDB backend and connection types, once.
///
/// Invoke once at the top of a domain crate's `lib.rs`. Needs `diesel` (with
/// its `mysql` feature) in the consumer's dependencies. A `MariaDB` server
/// uses diesel's `mysql` backend, so this macro covers it too. See
/// [`sqlite_backend!`] for the rationale.
#[macro_export]
macro_rules! mysql_backend {
    () => {
        /// The database backend, named once for the whole crate.
        ///
        /// The one place a diesel backend type appears:
        /// `#[entity(backend = crate::Backend)]` and `check_for_backend`
        /// refer here, so swapping backends is this call plus the connection
        /// URL.
        pub type Backend = ::diesel::mysql::Mysql;

        /// The connection type, following from [`Backend`].
        pub type Connection = ::diesel::mysql::MysqlConnection;
    };
}
