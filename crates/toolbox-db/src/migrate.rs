//! Migrations, serialised across replicas.
//!
//! `run_pending_migrations` is safe called once and a race when replicas start
//! together, which is what a rolling deploy does. So it runs under a
//! cluster-wide lock taken with the backend's own session primitive:
//! `pg_advisory_lock` on PostgreSQL, `GET_LOCK` on MySQL, nothing on SQLite
//! (one file, one writer, already serialised).
//!
//! This is not `toolbox-cluster`'s `LockManager`. That trait is async and
//! leased, and its shared adapter is built on this crate, so using it here
//! would invert the layering. Acquire, migrate and release all happen inside
//! one `Db::run` closure on one connection; anything recurring belongs on
//! `LockManager` instead.

use diesel::{
    QueryableByName, RunQueryDsl as _,
    connection::{Connection, LoadConnection},
    query_builder::SqlQuery,
    query_dsl::LoadQuery,
    sql_types::{BigInt, Nullable},
};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness};
use tracing::{info, warn};

use crate::error::{DbError, DbResult};

/// How long MySQL waits for the lock before giving up.
const MYSQL_LOCK_TIMEOUT_SECS: u32 = 120;

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
    C: MigrationLockConnection + MigrationHarness<<C as Connection>::Backend> + 'static,
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

/// A Diesel connection capable of taking the migration lock.
///
/// This hides the row shape MySQL's `GET_LOCK` returns from [`crate::Db`]'s
/// public bounds while keeping `toolbox-db` generic over the backend selected
/// by its consumer.
#[doc(hidden)]
pub trait MigrationLockConnection: Connection {
    /// Take the migration lock selected by `url`.
    fn acquire_migration_lock(&mut self, url: &str, name: &str) -> DbResult<()>;

    /// Release the migration lock selected by `url`.
    fn release_migration_lock(&mut self, url: &str, name: &str) -> DbResult<()>;
}

impl<C> MigrationLockConnection for C
where
    C: Connection + LoadConnection,
    for<'query> SqlQuery: LoadQuery<'query, C, MysqlLockResult>,
{
    fn acquire_migration_lock(&mut self, url: &str, name: &str) -> DbResult<()> {
        acquire(self, locking_for(url), name)
    }

    fn release_migration_lock(&mut self, url: &str, name: &str) -> DbResult<()> {
        release(self, locking_for(url), name)
    }
}

/// Which locking primitive a URL's backend offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Locking {
    /// `pg_advisory_lock`, held for the session.
    PostgresAdvisory,
    /// `GET_LOCK`, held for the session.
    MysqlNamed,
    /// None available, and none needed.
    None,
}

/// Pick the locking primitive from a connection URL.
///
/// # Arguments
///
/// * `url` - The connection URL. Only its scheme is read.
fn locking_for(url: &str) -> Locking {
    let lower = url.to_ascii_lowercase();
    if lower.starts_with("postgres://") || lower.starts_with("postgresql://") {
        Locking::PostgresAdvisory
    } else if lower.starts_with("mysql://") {
        Locking::MysqlNamed
    } else {
        // SQLite: one file with one writer, so concurrent work already
        // serialises on the file lock, and `busy_timeout` turns the contention
        // into a wait rather than an error.
        Locking::None
    }
}

/// Run `f` while holding a cluster-wide lock named `name`.
///
/// The lock is released even when `f` fails: holding a session lock past an
/// error would block every other replica until this connection is dropped.
///
/// **The whole critical section must be one closure on one connection.** These
/// are session locks, and a pool hands the connection back between statements.
///
/// # Arguments
///
/// * `conn` - The connection to hold the lock on. It must be the same one the
///   critical section runs against, because these are session locks.
/// * `url` - The connection URL, read only to pick the locking primitive.
/// * `name` - The lock name, shared by every replica that must be excluded.
/// * `f` - The critical section. It runs with the lock held, and the lock is
///   released whether it succeeds or fails.
///
/// # Errors
/// [`DbError::Migration`] when the lock cannot be taken, plus whatever `f`
/// returns.
fn with_lock<C, T, E>(
    conn: &mut C,
    url: &str,
    name: &str,
    f: impl FnOnce(&mut C) -> Result<T, E>,
) -> Result<T, E>
where
    C: MigrationLockConnection,
    E: From<DbError>,
{
    conn.acquire_migration_lock(url, name).map_err(E::from)?;
    let result = f(conn);
    if let Err(e) = conn.release_migration_lock(url, name) {
        warn!(error = %e, "could not release the `{name}` lock");
    }
    result
}

/// Take the lock, or do nothing on a backend that has none.
///
/// # Arguments
///
/// * `conn` - The connection that will hold the lock.
/// * `locking` - The primitive the backend offers.
/// * `name` - The lock name, hashed into an advisory key.
fn acquire<C>(conn: &mut C, locking: Locking, name: &str) -> DbResult<()>
where
    C: Connection + LoadConnection,
    for<'query> SqlQuery: LoadQuery<'query, C, MysqlLockResult>,
{
    match locking {
        Locking::PostgresAdvisory => conn
            .batch_execute(&format!("SELECT pg_advisory_lock({})", advisory_key(name)))
            .map_err(|e| DbError::Migration(e.to_string())),
        Locking::MysqlNamed => {
            let result = diesel::sql_query(format!(
                "SELECT GET_LOCK('{}', {MYSQL_LOCK_TIMEOUT_SECS}) AS acquired",
                mysql_lock_name(name)
            ))
            .get_result::<MysqlLockResult>(conn)
            .map_err(|e| DbError::Migration(e.to_string()))?;
            match result.acquired {
                Some(1) => Ok(()),
                Some(0) => Err(DbError::Migration(format!(
                    "timed out after {MYSQL_LOCK_TIMEOUT_SECS}s waiting for the migration lock"
                ))),
                other => Err(DbError::Migration(format!(
                    "MySQL GET_LOCK returned {other:?} for the migration lock"
                ))),
            }
        }
        Locking::None => Ok(()),
    }
}

/// The one-row result returned by MySQL's `GET_LOCK`.
#[derive(QueryableByName)]
struct MysqlLockResult {
    /// `1` acquired, `0` timed out, and `NULL` means an error.
    #[diesel(sql_type = Nullable<BigInt>)]
    acquired: Option<i64>,
}

/// Release the lock. Called on the error path too, so a failed critical section
/// does not block every other replica.
///
/// # Arguments
///
/// * `conn` - The connection holding the lock. It has to be the one that took
///   it.
/// * `locking` - The primitive the backend offers.
/// * `name` - The lock name.
fn release<C: Connection>(conn: &mut C, locking: Locking, name: &str) -> DbResult<()> {
    match locking {
        Locking::PostgresAdvisory => conn
            .batch_execute(&format!(
                "SELECT pg_advisory_unlock({})",
                advisory_key(name)
            ))
            .map_err(|e| DbError::Migration(e.to_string())),
        Locking::MysqlNamed => conn
            .batch_execute(&format!("SELECT RELEASE_LOCK('{}')", mysql_lock_name(name)))
            .map_err(|e| DbError::Migration(e.to_string())),
        Locking::None => Ok(()),
    }
}

/// A stable 64-bit key for a lock name, from FNV-1a.
///
/// Stable across processes and releases is the only property that matters: two
/// replicas must derive the same number from the same name. That rules out
/// `DefaultHasher`, whose output Rust is free to change between releases.
///
/// # Arguments
///
/// * `name` - The lock name. The same name must give the same number in every
///   process and every release.
fn advisory_key(name: &str) -> i64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    i64::from_ne_bytes(hash.to_ne_bytes())
}

/// The MySQL `GET_LOCK` name: the advisory key as fixed-width hex, so the
/// caller's string is hashed rather than interpolated (matching the Postgres
/// path) and the literal is always 16 characters of `[0-9a-f]`, well under
/// MySQL's 64-character limit.
///
/// # Arguments
///
/// * `name` - The caller's lock name, which never reaches SQL verbatim.
fn mysql_lock_name(name: &str) -> String {
    format!(
        "{:016x}",
        u64::from_ne_bytes(advisory_key(name).to_ne_bytes())
    )
}

#[cfg(test)]
mod tests {
    use super::{Locking, advisory_key, locking_for, mysql_lock_name};

    #[test]
    fn the_backend_is_read_from_the_url_scheme() {
        assert_eq!(locking_for("postgres://h/db"), Locking::PostgresAdvisory);
        assert_eq!(locking_for("postgresql://h/db"), Locking::PostgresAdvisory);
        assert_eq!(locking_for("POSTGRES://h/db"), Locking::PostgresAdvisory);
        assert_eq!(locking_for("mysql://h/db"), Locking::MysqlNamed);
        assert_eq!(locking_for("file.db"), Locking::None);
        assert_eq!(locking_for(":memory:"), Locking::None);
    }

    #[test]
    fn the_advisory_key_is_stable_and_name_dependent() {
        // Two replicas must derive the same number or the lock does nothing.
        assert_eq!(advisory_key("migrations"), advisory_key("migrations"));
        assert_ne!(advisory_key("a"), advisory_key("b"));
    }

    #[test]
    fn the_mysql_lock_name_is_hashed_never_interpolated() {
        // A name that would break or inject if it reached SQL verbatim.
        let hostile = "x', 0); DROP TABLE schema_migrations; -- ";
        let name = mysql_lock_name(hostile);
        assert_eq!(name.len(), 16);
        assert!(name.bytes().all(|b| b.is_ascii_hexdigit()));
        assert!(!name.contains("DROP TABLE"));
        assert_eq!(name, mysql_lock_name(hostile), "stable across calls");
    }
}
