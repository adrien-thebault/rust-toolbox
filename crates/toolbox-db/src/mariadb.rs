//! `MariaDB` connection setup.
//!
//! `MariaDB` converts `TIMESTAMP` values through the session time zone, so every
//! pooled connection must explicitly use UTC rather than inherit server state.

use diesel::{
    Connection,
    r2d2::{CustomizeConnection, Error},
};

/// Sets the `MariaDB` session time zone to UTC when a connection enters a pool.
#[derive(Debug, Clone, Copy, Default)]
pub struct MariaDbUtc;

/// Applies the UTC session invariant to every acquired connection.
///
/// Generic over the connection type because this crate has no backend feature.
/// Installing it on a pool that is not `MariaDB` makes connection acquisition
/// fail, which is why [`crate::DbBuilder::mariadb_utc`] is explicit.
impl<C> CustomizeConnection<C, Error> for MariaDbUtc
where
    C: Connection + 'static,
{
    fn on_acquire(&self, conn: &mut C) -> Result<(), Error> {
        conn.batch_execute("SET time_zone = '+00:00';")
            .map_err(Error::QueryError)
    }
}
