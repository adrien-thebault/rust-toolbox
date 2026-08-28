//! One harness per crate.
//!
//! The behavioural tests need a real PostgreSQL and soft-skip without one, so
//! `TOOLBOX_TEST_POSTGRES_URL` is what turns them on. Everything that can be
//! asserted without a server is asserted unconditionally.
#![allow(missing_docs, clippy::missing_panics_doc)]

mod adapters;

use diesel::pg::PgConnection;
use toolbox_db::Db;

/// A pool against the test server, or `None` when there is not one.
pub fn test_db() -> Option<Db<PgConnection>> {
    let url = std::env::var("TOOLBOX_TEST_POSTGRES_URL").ok()?;
    Db::<PgConnection>::builder(url)
        .max_size(4)
        .connect_timeout(std::time::Duration::from_secs(2))
        .build()
        .ok()
}

/// The outbox is one shared queue, so tests that drain it cannot run at the
/// same time as each other without stealing each other's events.
pub static OUTBOX: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

/// Skip a test that needs a server, saying so rather than silently passing.
#[macro_export]
macro_rules! require_postgres {
    () => {
        match $crate::test_db() {
            Some(db) => db,
            None => {
                eprintln!("skipping: TOOLBOX_TEST_POSTGRES_URL is not set");
                return;
            }
        }
    };
}
