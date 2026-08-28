//! Shared cluster adapters over PostgreSQL.
//!
//! These are the shared adapters `toolbox-cluster` requires. A trait with only
//! a local adapter has never been tested against the thing it abstracts.
//!
//! PostgreSQL and nothing else, deliberately: if you are running more than one
//! replica you already have it, so this is the zero-new-infrastructure option.
//! Redis, NATS and Kafka are a day each **after** this, because by then an
//! adapter is an implementation of a trait whose contract is already pinned by
//! tests.
//!
//! # Why this is a separate crate rather than a feature
//!
//! The plan put these behind a `postgres` feature on `toolbox-cluster`. They
//! cannot go there. These adapters name `diesel::pg::Pg` concretely - raw SQL
//! that returns rows needs a concrete backend, per
//! backend - so the feature would enable
//! `diesel/postgres`, and cargo unifies features across a workspace. Every
//! sibling crate would then compile PostgreSQL support and need `libpq`
//! present, including a gateway that only ever talks to SQLite.
//!
//! Forcing that on a sibling that never asked for it is what makes this a
//! crate rather than a feature.

pub mod key_value;
pub mod lock;
pub mod outbox;
pub mod schema;

pub use key_value::PostgresKeyValue;
pub use lock::PostgresLocks;
pub use outbox::OutboxBus;

/// This crate's migrations. **You** call `db.migrate(MIGRATIONS)`.
pub const MIGRATIONS: toolbox_db::EmbeddedMigrations = toolbox_db::embed_migrations!("migrations");
