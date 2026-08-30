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
//! The `chrono`, `time` and `clap` features here add [`Now`] impls and the
//! clap argument struct. They are not backend selectors.

pub mod args;
pub mod db;
pub mod entity;
pub mod error;
pub mod migrate;
pub mod pagination;
pub mod sqlite;

pub use db::{Db, DbBuilder, DbPool, DbPooledConn};
/// Re-exported so `#[derive(Entity)]` can name it without the consumer
/// declaring `diesel_migrations` itself.
pub use diesel_migrations::{EmbeddedMigrations, embed_migrations};
pub use entity::{Entity, Now};
pub use error::{DbError, DbResult};
pub use pagination::{Paginate, Paginated};
pub use sqlite::SqlitePragmas;
/// The derive. Shares its name with the [`Entity`] trait it implements, the
/// way `serde::Serialize` does: derives live in the macro namespace.
pub use toolbox_macros::Entity;
