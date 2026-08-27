//! Migrations, serialised across replicas.
//!
//! `run_pending_migrations` is safe to call once and a race when three
//! replicas start together, which is exactly what a rolling deploy does. The
//! lock comes from [`crate::lock`], and it is taken with the backend's own
//! primitive rather than through `LockManager`, because that trait's PostgreSQL
//! adapter is built on this crate and the dependency would be circular.

use diesel::connection::Connection;
use diesel_migrations::{EmbeddedMigrations, MigrationHarness};
use tracing::info;

use crate::{
    error::{DbError, DbResult},
    lock::with_lock,
};

/// The lock every replica contends on while migrating.
pub const LOCK_NAME: &str = "toolbox_migrations";

/// Run every pending migration, holding a cross-replica lock while doing it.
///
/// # Arguments
///
/// * `conn` - The connection to migrate on, which also holds the lock.
/// * `url` - The connection URL, read only to pick the locking primitive.
/// * `migrations` - The embedded migration set to apply.
///
/// # Errors
/// [`DbError::Migration`] when the lock cannot be taken or a migration fails.
pub fn run_locked<C>(conn: &mut C, url: &str, migrations: EmbeddedMigrations) -> DbResult<()>
where
    C: Connection + MigrationHarness<<C as Connection>::Backend> + 'static,
{
    with_lock(conn, url, LOCK_NAME, |conn| {
        conn.run_pending_migrations(migrations)
            .map(|versions| {
                for v in versions {
                    info!(version = %v, "applied migration");
                }
            })
            .map_err(|e| DbError::Migration(e.to_string()))
    })
}
