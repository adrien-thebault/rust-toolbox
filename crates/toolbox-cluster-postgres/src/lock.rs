//! Distributed locks over a lease table.
//!
//! It removes a trap that looks like the obvious implementation and is not.
//!
//! # Why not `pg_advisory_lock`
//!
//! An advisory lock belongs to a **session**. `Db::run` returns its connection
//! to the pool between statements, so the next caller can be handed the same
//! session - and advisory locks are re-entrant within a session, so that caller
//! takes a lock somebody else is holding, and both proceed. The first version
//! of this file did exactly that, and a test against a real server caught it
//! within a minute.
//!
//! Holding a dedicated connection for the lock's lifetime would fix it, at the
//! cost of one pool connection per held lock and no lease at all - a holder
//! that hangs keeps the lock until its connection dies.
//!
//! A lease row has neither problem. It is owned by whoever wrote it, whichever
//! connection they happen to be using, and it expires on its own.
//!
//! `pg_advisory_lock` is still right for `toolbox_db`'s migration lock, where
//! acquire, migrate and release all happen inside **one** `Db::run` closure and
//! therefore on one connection.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use diesel::{pg::PgConnection, prelude::*};
use toolbox_cluster::{
    deployment::{Adapter, Scope},
    lock::{LockCapabilities, LockError, LockGuard, LockManager, LockRelease},
};
use toolbox_db::Db;
use tracing::warn;

use crate::schema::toolbox_locks;

/// Locks every replica shares, held as leases.
#[derive(Clone)]
pub struct PostgresLocks {
    db: Db<PgConnection>,
}

impl std::fmt::Debug for PostgresLocks {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PostgresLocks")
    }
}

impl PostgresLocks {
    /// Build over a pool.
    ///
    /// # Arguments
    ///
    /// * `db` - The pool holding the lease table.
    #[must_use]
    pub fn new(db: Db<PgConnection>) -> Self {
        Self { db }
    }

    /// Extend a lease this owner still holds.
    ///
    /// A job that runs longer than its lease renews rather than raising the
    /// lease: a long lease is also a long outage when the holder dies.
    ///
    /// # Arguments
    ///
    /// * `key` - The lock being held.
    /// * `owner` - The holder. A renewal by anyone else is refused, which is
    ///   how a lease that already expired stays expired.
    /// * `lease` - How much longer to hold it, measured from now rather than
    ///   added to the old expiry.
    ///
    /// # Errors
    /// [`LockError::Backend`] when the statement fails.
    pub async fn renew(&self, key: &str, owner: &str, lease: Duration) -> Result<bool, LockError> {
        let (key, owner) = (key.to_owned(), owner.to_owned());
        let until = chrono::Utc::now() + to_chrono(lease);

        let changed = self
            .db
            .query(move |c: &mut PgConnection| {
                diesel::update(
                    toolbox_locks::table
                        .filter(toolbox_locks::key.eq(&key))
                        .filter(toolbox_locks::owner.eq(&owner)),
                )
                .set(toolbox_locks::expires_at.eq(until))
                .execute(c)
            })
            .await
            .map_err(|e| LockError::Backend(e.to_string()))?;
        Ok(changed > 0)
    }

    /// Delete every lease that has expired.
    ///
    /// Housekeeping: an expired lease is already takeable, so this only keeps
    /// the table from accumulating rows for keys nobody uses any more.
    ///
    /// # Errors
    /// [`LockError::Backend`] when the statement fails.
    pub async fn purge_expired(&self) -> Result<usize, LockError> {
        let now = chrono::Utc::now();
        self.db
            .query(move |c: &mut PgConnection| {
                diesel::delete(toolbox_locks::table.filter(toolbox_locks::expires_at.lt(now)))
                    .execute(c)
            })
            .await
            .map_err(|e| LockError::Backend(e.to_string()))
    }
}

/// A standard `Duration` as a chrono one, for the interval arithmetic the lease
/// statement does.
///
/// # Arguments
///
/// * `d` - The lease length. Saturates rather than panicking on a duration
///   chrono cannot represent.
fn to_chrono(d: Duration) -> chrono::Duration {
    chrono::Duration::from_std(d).unwrap_or_else(|_| chrono::Duration::hours(1))
}

struct Release {
    db: Db<PgConnection>,
}

impl LockRelease for Release {
    fn release(&self, key: &str, owner: &str) {
        let db = self.db.clone();
        let (key, owner) = (key.to_owned(), owner.to_owned());

        // Drop cannot await, so this is spawned. A missed release is not a
        // leak: the lease expires on its own, which is the whole reason it is
        // a lease.
        tokio::spawn(async move {
            let result = db
                .query(move |c: &mut PgConnection| {
                    // Only if we still hold it. The lease may have expired and
                    // been taken by somebody else, and deleting then would
                    // steal their lock.
                    diesel::delete(
                        toolbox_locks::table
                            .filter(toolbox_locks::key.eq(&key))
                            .filter(toolbox_locks::owner.eq(&owner)),
                    )
                    .execute(c)
                })
                .await;
            if let Err(e) = result {
                warn!(error = %e, "could not release a lock; its lease will expire");
            }
        });
    }
}

#[async_trait]
impl LockManager for PostgresLocks {
    fn capabilities(&self) -> LockCapabilities {
        LockCapabilities {
            shared: true,
            leased: true,
        }
    }

    async fn try_lock(&self, key: &str, lease: Duration) -> Result<Option<LockGuard>, LockError> {
        let owner = uuid::Uuid::now_v7().to_string();
        let name = key.to_owned();
        let key_owned = key.to_owned();
        let owner_owned = owner.clone();
        // A lease longer than a day is a configuration mistake rather than an
        // intention, and f64 is what make_interval takes.
        let seconds = lease.as_secs_f64().min(86_400.0);

        // One statement, so two replicas cannot both believe they won. The
        // WHERE on the DO UPDATE is what makes it a *try*: it only takes over
        // a lease that has already expired. Written out rather than built with
        // the dsl because diesel's ON CONFLICT builder does not carry a WHERE.
        //
        // The expiry is computed by the database, so a replica whose clock is
        // wrong cannot hold a lease longer or shorter than it asked for.
        let holder = self
            .db
            .query(move |c: &mut PgConnection| {
                diesel::sql_query(
                    "INSERT INTO toolbox_locks (key, owner, expires_at) \
                     VALUES ($1, $2, now() + make_interval(secs => $3)) \
                     ON CONFLICT (key) DO UPDATE \
                     SET owner = EXCLUDED.owner, expires_at = EXCLUDED.expires_at \
                     WHERE toolbox_locks.expires_at < now() \
                     RETURNING owner",
                )
                .bind::<diesel::sql_types::Text, _>(key_owned)
                .bind::<diesel::sql_types::Text, _>(owner_owned)
                .bind::<diesel::sql_types::Double, _>(seconds)
                .get_result::<Holder>(c)
                .optional()
            })
            .await
            .map_err(|e| LockError::Backend(e.to_string()))?;

        // No row came back means the key existed and the WHERE rejected the
        // update: somebody else holds an unexpired lease.
        if holder.map(|h| h.owner).as_deref() != Some(owner.as_str()) {
            return Ok(None);
        }

        Ok(Some(LockGuard::new(
            name,
            owner,
            Arc::new(Release {
                db: self.db.clone(),
            }),
        )))
    }
}

/// The owner returned by the take-the-lease statement.
#[derive(diesel::QueryableByName)]
struct Holder {
    #[diesel(sql_type = diesel::sql_types::Text)]
    owner: String,
}

impl Adapter for PostgresLocks {
    fn name(&self) -> &'static str {
        "PostgresLocks"
    }

    fn scope(&self) -> Scope {
        Scope::Shared
    }
}
