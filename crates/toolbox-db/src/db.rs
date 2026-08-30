//! The pool handle, and the reason this crate exists.
//!
//! It makes the blocking path unreachable from the ergonomic API - every diesel
//! call from `async` code goes through a closure this type runs on a blocking
//! thread - and it unifies r2d2's, diesel's and tokio's error types into one
//! `DbError`.
//!
//! **The pool is a swappable internal decision.** `Db<C>`'s public API is the
//! same over `diesel::r2d2` or over `deadpool-diesel`; the safety property
//! comes from `run`, not from the pool. If waiting for a pool permit on a
//! blocking thread ever shows up in a profile, swap the internals and no
//! consumer notices.

use std::{sync::Arc, time::Duration};

use diesel::r2d2::{ConnectionManager, CustomizeConnection, Error as R2d2Error, R2D2Connection};
use diesel_migrations::{EmbeddedMigrations, MigrationHarness};

use crate::{
    error::{DbError, DbResult},
    migrate,
    sqlite::SqlitePragmas,
};

/// The connection pool backing a [`Db`].
pub type DbPool<C> = diesel::r2d2::Pool<ConnectionManager<C>>;

/// A connection checked out of the pool.
pub type DbPooledConn<C> = diesel::r2d2::PooledConnection<ConnectionManager<C>>;

/// A database handle.
///
/// Clone it freely: clones share one pool.
pub struct Db<C: R2D2Connection + 'static> {
    /// The shared r2d2 pool.
    pool: DbPool<C>,
    /// The connection URL, kept for diagnostics. Carries credentials.
    url: Arc<str>,
}

impl<C: R2D2Connection + 'static> Clone for Db<C> {
    fn clone(&self) -> Self {
        Self {
            pool: self.pool.clone(),
            url: Arc::clone(&self.url),
        }
    }
}

impl<C: R2D2Connection + 'static> std::fmt::Debug for Db<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Never the URL: it carries the password.
        f.debug_struct("Db")
            .field("connections", &self.pool.state().connections)
            .finish_non_exhaustive()
    }
}

impl<C: R2D2Connection + 'static> Db<C> {
    /// A pool with the default settings.
    ///
    /// # Arguments
    ///
    /// * `url` - The connection URL. Its scheme also decides which locking
    ///   primitive `migrate` uses.
    ///
    /// # Errors
    /// [`DbError::Pool`] when the first connection cannot be established.
    pub fn new(url: impl Into<String>) -> DbResult<Self> {
        Self::builder(url).build()
    }

    /// A pool to configure before building.
    ///
    /// # Arguments
    ///
    /// * `url` - The connection URL, as for [`Db::new`].
    pub fn builder(url: impl Into<String>) -> DbBuilder<C> {
        DbBuilder {
            url: url.into(),
            max_size: None,
            min_idle: None,
            connect_timeout: None,
            customizer: None,
        }
    }

    /// Run a closure against a pooled connection, off the async runtime.
    ///
    /// This is the whole point of the type: `f` is blocking diesel code, and it
    /// runs on a blocking thread rather than parking a tokio worker. Both
    /// checking out the connection and running the query happen there.
    ///
    /// # Arguments
    ///
    /// * `f` - The blocking diesel code. It receives a pooled connection and
    ///   runs on a blocking thread, never on the async runtime.
    ///
    /// # Errors
    /// Whatever `f` returns, or a [`DbError`] converted into `E` when the pool
    /// or the blocking task itself fails.
    pub async fn run<T, E, F>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce(&mut C) -> Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: From<DbError> + Send + 'static,
    {
        let pool = self.pool.clone();
        let joined = tokio::task::spawn_blocking(move || {
            let mut conn = pool.get().map_err(|e| E::from(DbError::Pool(e)))?;
            f(&mut conn)
        })
        .await;

        match joined {
            Ok(result) => result,
            Err(e) => Err(E::from(DbError::Interact(e.to_string()))),
        }
    }

    /// Run a closure that returns diesel's own `QueryResult`.
    ///
    /// The common shape: `run` needs `E: From<DbError>`, which
    /// `diesel::result::Error` cannot implement, so a closure built straight
    /// out of diesel's dsl does not fit it. This is that closure, with the
    /// error mapped for you.
    ///
    /// # Arguments
    ///
    /// * `f` - The blocking diesel code, returning diesel's own `QueryResult`
    ///   rather than a domain error.
    ///
    /// # Errors
    /// [`DbError::Query`] for a failed statement, or [`DbError::Pool`] /
    /// [`DbError::Interact`] as [`Db::run`].
    pub async fn query<T, F>(&self, f: F) -> DbResult<T>
    where
        F: FnOnce(&mut C) -> diesel::QueryResult<T> + Send + 'static,
        T: Send + 'static,
    {
        self.run(move |conn| f(conn).map_err(DbError::from)).await
    }

    /// As [`Db::query`], inside a span named for the operation.
    ///
    /// # Arguments
    ///
    /// * `name` - The operation name for the span. A `&'static str`, so it
    ///   cannot carry per-request data into a span field.
    /// * `f` - The blocking diesel code, as for [`Db::query`].
    ///
    /// # Errors
    /// As [`Db::query`].
    #[tracing::instrument(level = "debug", skip_all, fields(db.op = %name))]
    pub async fn query_named<T, F>(&self, name: &'static str, f: F) -> DbResult<T>
    where
        F: FnOnce(&mut C) -> diesel::QueryResult<T> + Send + 'static,
        T: Send + 'static,
    {
        self.query(f).await
    }

    /// Run a closure inside a transaction.
    ///
    /// Rollback semantics are diesel's: `f` returning `Err` rolls back, and a
    /// panic rolls back too. `E` needs `From<diesel::result::Error>` on top of
    /// `run`'s bound because that is what `Connection::transaction` requires;
    /// requires.
    ///
    /// # Arguments
    ///
    /// * `f` - The blocking diesel code. Returning `Err` rolls back, and so
    ///   does a panic.
    ///
    /// # Errors
    /// Whatever `f` returns, or a [`DbError`] converted into `E`.
    pub async fn transaction<T, E, F>(&self, f: F) -> Result<T, E>
    where
        F: FnOnce(&mut C) -> Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: From<DbError> + From<diesel::result::Error> + Send + 'static,
    {
        self.run(move |conn| conn.transaction(f)).await
    }

    /// As [`Db::run`], inside a span named for the operation.
    ///
    /// `#[derive(Entity)]` calls this. One span at DEBUG replaces the dozen
    /// hand-written `info!` lines the old service traits emitted, which logged
    /// every read at INFO.
    ///
    /// # Arguments
    ///
    /// * `name` - The operation name for the span, which is what
    ///   `#[derive(Entity)]` fills in.
    /// * `f` - The blocking diesel code, as for [`Db::run`].
    ///
    /// # Errors
    /// As [`Db::run`].
    #[tracing::instrument(level = "debug", skip_all, fields(db.op = %name))]
    pub async fn run_named<T, E, F>(&self, name: &'static str, f: F) -> Result<T, E>
    where
        F: FnOnce(&mut C) -> Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: From<DbError> + Send + 'static,
    {
        self.run(f).await
    }

    /// As [`Db::transaction`], inside a span named for the operation.
    ///
    /// # Arguments
    ///
    /// * `name` - The operation name for the span.
    /// * `f` - The blocking diesel code, as for [`Db::transaction`].
    ///
    /// # Errors
    /// As [`Db::transaction`].
    #[tracing::instrument(level = "debug", skip_all, fields(db.op = %name))]
    pub async fn transaction_named<T, E, F>(&self, name: &'static str, f: F) -> Result<T, E>
    where
        F: FnOnce(&mut C) -> Result<T, E> + Send + 'static,
        T: Send + 'static,
        E: From<DbError> + From<diesel::result::Error> + Send + 'static,
    {
        self.transaction(f).await
    }

    /// Apply every pending migration, holding a cross-replica lock.
    ///
    /// # Arguments
    ///
    /// * `migrations` - The embedded migration set, from `embed_migrations!` in
    ///   the crate that owns the SQL.
    ///
    /// # Errors
    /// [`DbError::Migration`] when the lock cannot be taken or a migration
    /// fails.
    pub async fn migrate(&self, migrations: EmbeddedMigrations) -> DbResult<()>
    where
        C: MigrationHarness<<C as diesel::Connection>::Backend>,
    {
        let url = self.url.to_string();
        self.run(move |conn| migrate::run_locked(conn, &url, migrations))
            .await
    }

    /// Check out a connection to use from blocking code.
    ///
    /// The escape hatch, named so it shows up in review. Calling this from an
    /// `async fn` parks a runtime worker for as long as the pool takes to
    /// answer, which is the bug the rest of this type exists to prevent.
    ///
    /// # Errors
    /// [`DbError::Pool`] when the pool cannot hand out a connection in time.
    pub fn blocking_conn(&self) -> DbResult<DbPooledConn<C>> {
        Ok(self.pool.get()?)
    }

    /// The underlying pool, for code that needs r2d2 directly.
    pub fn pool(&self) -> &DbPool<C> {
        &self.pool
    }

    /// The connection URL. Carries credentials: never log it.
    pub fn url(&self) -> &str {
        &self.url
    }
}

/// Configures a [`Db`] before building it.
pub struct DbBuilder<C: R2D2Connection + 'static> {
    /// The connection URL.
    url: String,
    /// Pool ceiling, if overridden.
    max_size: Option<u32>,
    /// Idle connections to keep warm, if set.
    min_idle: Option<u32>,
    /// How long to wait for a connection, if set.
    connect_timeout: Option<Duration>,
    /// A per-connection setup hook, if any.
    customizer: Option<Box<dyn CustomizeConnection<C, R2d2Error>>>,
}

impl<C: R2D2Connection + 'static> std::fmt::Debug for DbBuilder<C> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DbBuilder")
            .field("max_size", &self.max_size)
            .field("min_idle", &self.min_idle)
            .field("connect_timeout", &self.connect_timeout)
            .finish_non_exhaustive()
    }
}

impl<C: R2D2Connection + 'static> DbBuilder<C> {
    /// The largest number of connections the pool will open.
    ///
    /// # Arguments
    ///
    /// * `n` - The connection ceiling. On SQLite keep it at one unless the
    ///   pragmas set a busy timeout, because WAL allows only one writer.
    #[must_use]
    pub fn max_size(mut self, n: u32) -> Self {
        self.max_size = Some(n);
        self
    }

    /// The number of idle connections the pool keeps warm.
    ///
    /// # Arguments
    ///
    /// * `n` - How many connections to keep warm, so a burst does not pay
    ///   connection setup.
    #[must_use]
    pub fn min_idle(mut self, n: u32) -> Self {
        self.min_idle = Some(n);
        self
    }

    /// How long `run` waits for a connection before failing.
    ///
    /// # Arguments
    ///
    /// * `d` - How long to wait for a free connection before returning
    ///   [`DbError::Pool`].
    #[must_use]
    pub fn connect_timeout(mut self, d: Duration) -> Self {
        self.connect_timeout = Some(d);
        self
    }

    /// Apply SQLite pragmas to every connection.
    ///
    /// Only call this on a SQLite pool: the pragmas are SQLite syntax, so a
    /// PostgreSQL pool would fail to hand out connections.
    ///
    /// # Arguments
    ///
    /// * `pragmas` - The pragmas to apply on every checkout. SQLite syntax, so
    ///   this must not be set on another backend.
    #[must_use]
    pub fn sqlite_pragmas(mut self, pragmas: SqlitePragmas) -> Self {
        self.customizer = Some(Box::new(pragmas));
        self
    }

    /// Apply an arbitrary r2d2 connection customizer.
    ///
    /// # Arguments
    ///
    /// * `c` - An r2d2 customizer, run once per connection as it enters the
    ///   pool.
    #[must_use]
    pub fn customizer(mut self, c: Box<dyn CustomizeConnection<C, R2d2Error>>) -> Self {
        self.customizer = Some(c);
        self
    }

    /// Build the pool.
    ///
    /// # Errors
    /// [`DbError::Pool`] when the first connection cannot be
    /// established.
    pub fn build(self) -> DbResult<Db<C>> {
        let mut builder = diesel::r2d2::Pool::builder();
        if let Some(n) = self.max_size {
            builder = builder.max_size(n);
        }
        if let Some(n) = self.min_idle {
            builder = builder.min_idle(Some(n));
        }
        if let Some(d) = self.connect_timeout {
            builder = builder.connection_timeout(d);
        }
        if let Some(c) = self.customizer {
            builder = builder.connection_customizer(c);
        }
        let pool = builder.build(ConnectionManager::<C>::new(self.url.as_str()))?;
        Ok(Db {
            pool,
            url: Arc::from(self.url),
        })
    }
}
