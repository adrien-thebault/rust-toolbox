use diesel::{RunQueryDsl as _, sql_query, sqlite::SqliteConnection};
use toolbox_db::{EmbeddedMigrations, embed_migrations};

use crate::fixtures::temp_db;

const MIGRATIONS: EmbeddedMigrations = embed_migrations!("tests/migrations");

/// `Db::migrate` takes the cross-replica lock, applies the pending set on the
/// one connection, and releases it. On SQLite the lock itself is a no-op, but
/// every other step - the harness table, the pending scan, the apply loop -
/// still runs, which is the path a single-node deploy exercises.
#[tokio::test]
async fn migrate_applies_the_pending_set_and_is_idempotent() {
    let (db, _dir) = temp_db();

    db.migrate(MIGRATIONS).await.expect("first run applies");

    // The migration created `widgets`; a row inserts only if it ran.
    db.query(|conn: &mut SqliteConnection| {
        sql_query("INSERT INTO widgets (id, name) VALUES (1, 'a')").execute(conn)
    })
    .await
    .expect("the migration created widgets");

    // Nothing pending now: the lock/acquire/release path runs again and returns
    // cleanly rather than erroring on the already-applied migration.
    db.migrate(MIGRATIONS).await.expect("second run is a no-op");
}
