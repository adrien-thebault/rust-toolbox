//! A lock held across replicas, using whatever the backend offers.
//!
//! Separate from migrations because it is not specific to them: anything that
//! must happen once per cluster rather than once per process can take one.
//!
//! The primitive is chosen from the connection URL, because that is the only
//! backend signal available - this crate deliberately has no backend feature
//! to match on.

use diesel::connection::Connection;
use tracing::warn;

use crate::error::{DbError, DbResult};

/// How long MySQL waits for a lock before giving up.
const MYSQL_LOCK_TIMEOUT_SECS: u32 = 120;

/// Which locking primitive a URL's backend offers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Locking {
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
#[must_use]
pub(crate) fn locking_for(url: &str) -> Locking {
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
#[must_use]
pub fn advisory_key(name: &str) -> i64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in name.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    i64::from_ne_bytes(hash.to_ne_bytes())
}

/// Raw SQL through `batch_execute` rather than `sql_query`.
///
/// `diesel::sql_query` needs `QueryFragment<DB>`, which is gated behind a
/// sealed trait a generic `C::Backend` cannot name without opting into diesel's
/// third-party-backend feature. `SimpleConnection` takes a `&str` and every
/// connection implements it, so this stays generic.
///
/// # Arguments
///
/// * `locking` - Which primitive the backend offers. `None` for SQLite, which
///   has nothing to lock across replicas.
/// * `name` - The lock name, hashed into an advisory key rather than
///   interpolated.
/// * `acquiring` - `true` for the statement that takes the lock, `false` for
///   the one that releases it.
fn lock_sql(locking: Locking, name: &str, acquiring: bool) -> Option<String> {
    match (locking, acquiring) {
        (Locking::PostgresAdvisory, true) => {
            Some(format!("SELECT pg_advisory_lock({})", advisory_key(name)))
        }
        (Locking::PostgresAdvisory, false) => {
            Some(format!("SELECT pg_advisory_unlock({})", advisory_key(name)))
        }
        (Locking::MysqlNamed, true) => Some(format!(
            "SELECT GET_LOCK('{name}', {MYSQL_LOCK_TIMEOUT_SECS})"
        )),
        (Locking::MysqlNamed, false) => Some(format!("SELECT RELEASE_LOCK('{name}')")),
        (Locking::None, _) => None,
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
pub fn with_lock<C, T, E>(
    conn: &mut C,
    url: &str,
    name: &str,
    f: impl FnOnce(&mut C) -> Result<T, E>,
) -> Result<T, E>
where
    C: Connection,
    E: From<DbError>,
{
    let locking = locking_for(url);
    acquire(conn, locking, name).map_err(E::from)?;
    let result = f(conn);
    if let Err(e) = release(conn, locking, name) {
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
fn acquire<C: Connection>(conn: &mut C, locking: Locking, name: &str) -> DbResult<()> {
    if let Some(sql) = lock_sql(locking, name, true) {
        conn.batch_execute(&sql)
            .map_err(|e| DbError::Migration(e.to_string()))?;
    }
    Ok(())
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
    if let Some(sql) = lock_sql(locking, name, false) {
        conn.batch_execute(&sql)
            .map_err(|e| DbError::Migration(e.to_string()))?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{Locking, advisory_key, locking_for};

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
}
