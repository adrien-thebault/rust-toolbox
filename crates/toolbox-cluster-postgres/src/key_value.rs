//! A key-value store in PostgreSQL.

use std::time::Duration;

use async_trait::async_trait;
use diesel::{pg::PgConnection, prelude::*};
use toolbox_cluster::{
    KeyValueCapabilities, KeyValueError, KeyValueStore,
    deployment::{Adapter, Scope},
};
use toolbox_db::Db;

use crate::schema::toolbox_kv;

/// A key-value store every replica shares.
#[derive(Clone)]
pub struct PostgresKeyValue {
    /// The shared pool the migrations were applied to.
    db: Db<PgConnection>,
}

impl std::fmt::Debug for PostgresKeyValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PostgresKeyValue")
    }
}

impl PostgresKeyValue {
    /// Build over a pool.
    ///
    /// # Arguments
    ///
    /// * `db` - The pool. It must reach the database the migrations in this
    ///   crate were applied to.
    #[must_use]
    pub fn new(db: Db<PgConnection>) -> Self {
        Self { db }
    }

    /// Delete every expired row.
    ///
    /// Expiry is enforced on read, so this is housekeeping rather than
    /// correctness - but without it the table grows forever with keys nobody
    /// will ask for again.
    ///
    /// # Errors
    /// [`KeyValueError::Backend`] when the statement fails.
    pub async fn purge_expired(&self) -> Result<usize, KeyValueError> {
        let now = chrono::Utc::now();
        self.db
            .query(move |c: &mut PgConnection| {
                diesel::delete(toolbox_kv::table.filter(toolbox_kv::expires_at.lt(now))).execute(c)
            })
            .await
            .map_err(|e| KeyValueError::Backend(e.to_string()))
    }
}

#[async_trait]
impl KeyValueStore for PostgresKeyValue {
    fn capabilities(&self) -> KeyValueCapabilities {
        KeyValueCapabilities {
            atomic_take: true,
            atomic_add: true,
            ttl: true,
            durable: true,
            shared: true,
        }
    }

    async fn get(&self, key: &str) -> Result<Option<Vec<u8>>, KeyValueError> {
        let key = key.to_owned();
        let now = chrono::Utc::now();
        self.db
            .query(move |c: &mut PgConnection| {
                toolbox_kv::table
                    .filter(toolbox_kv::key.eq(&key))
                    // Expiry is enforced on read as well as by the purge, so a
                    // stale row is never returned even between purges.
                    .filter(
                        toolbox_kv::expires_at
                            .is_null()
                            .or(toolbox_kv::expires_at.gt(now)),
                    )
                    .select(toolbox_kv::value)
                    .first::<Vec<u8>>(c)
                    .optional()
            })
            .await
            .map_err(|e| KeyValueError::Backend(e.to_string()))
    }

    async fn set(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Option<Duration>,
    ) -> Result<(), KeyValueError> {
        let key = key.to_owned();
        let expires_at = ttl.map(|d| {
            chrono::Utc::now()
                + chrono::Duration::from_std(d).unwrap_or_else(|_| chrono::Duration::days(365))
        });

        self.db
            .query(move |c: &mut PgConnection| {
                diesel::insert_into(toolbox_kv::table)
                    .values((
                        toolbox_kv::key.eq(&key),
                        toolbox_kv::value.eq(&value),
                        toolbox_kv::expires_at.eq(expires_at),
                    ))
                    .on_conflict(toolbox_kv::key)
                    .do_update()
                    .set((
                        toolbox_kv::value.eq(&value),
                        toolbox_kv::expires_at.eq(expires_at),
                    ))
                    .execute(c)
            })
            .await
            .map(|_| ())
            .map_err(|e| KeyValueError::Backend(e.to_string()))
    }

    async fn add(
        &self,
        key: &str,
        value: Vec<u8>,
        ttl: Option<Duration>,
    ) -> Result<bool, KeyValueError> {
        use diesel::sql_types::{Binary, Nullable, Text, Timestamptz};

        let key = key.to_owned();
        let expires_at = ttl.map(|d| {
            chrono::Utc::now()
                + chrono::Duration::from_std(d).unwrap_or_else(|_| chrono::Duration::days(365))
        });

        self.db
            .query(move |c: &mut PgConnection| {
                // One statement: insert when the key is free, overwrite it when
                // the existing row has expired, change nothing when a live row
                // holds it. The affected-row count is 1 only in the first two
                // cases. A typed builder cannot express a `WHERE` on the
                // conflict action without shadowing `QueryDsl::filter`.
                diesel::sql_query(
                    "INSERT INTO toolbox_kv (key, value, expires_at) VALUES ($1, $2, $3) \
                     ON CONFLICT (key) DO UPDATE \
                       SET value = EXCLUDED.value, expires_at = EXCLUDED.expires_at \
                       WHERE toolbox_kv.expires_at IS NOT NULL \
                         AND toolbox_kv.expires_at <= now()",
                )
                .bind::<Text, _>(key)
                .bind::<Binary, _>(value)
                .bind::<Nullable<Timestamptz>, _>(expires_at)
                .execute(c)
            })
            .await
            .map(|n| n == 1)
            .map_err(|e| KeyValueError::Backend(e.to_string()))
    }

    async fn take(&self, key: &str) -> Result<Option<Vec<u8>>, KeyValueError> {
        let key = key.to_owned();
        let now = chrono::Utc::now();
        self.db
            .query(move |c: &mut PgConnection| {
                // One statement. DELETE .. RETURNING is atomic, so two callers
                // cannot both receive the value - which is the whole property
                // refresh-token rotation is built on.
                diesel::delete(
                    toolbox_kv::table.filter(toolbox_kv::key.eq(&key)).filter(
                        toolbox_kv::expires_at
                            .is_null()
                            .or(toolbox_kv::expires_at.gt(now)),
                    ),
                )
                .returning(toolbox_kv::value)
                .get_result::<Vec<u8>>(c)
                .optional()
            })
            .await
            .map_err(|e| KeyValueError::Backend(e.to_string()))
    }

    async fn delete(&self, key: &str) -> Result<(), KeyValueError> {
        let key = key.to_owned();
        self.db
            .query(move |c: &mut PgConnection| {
                diesel::delete(toolbox_kv::table.filter(toolbox_kv::key.eq(&key))).execute(c)
            })
            .await
            .map(|_| ())
            .map_err(|e| KeyValueError::Backend(e.to_string()))
    }
}

impl Adapter for PostgresKeyValue {
    fn name(&self) -> &'static str {
        "PostgresKeyValue"
    }

    fn scope(&self) -> Scope {
        Scope::Shared
    }
}
