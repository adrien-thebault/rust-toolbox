//! `#[derive(Entity)]` against a real database.
//!
//! The trybuild cases in `toolbox-macros` cover what the derive *rejects*;
//! this covers what it generates.

use diesel::{connection::SimpleConnection, prelude::*, sqlite::SqliteConnection};
use toolbox_core::{PageRequest, Sort};
use toolbox_db::{Db, DbError, Entity as _};

/// The backend, named once, exactly as the template does it.
pub type Backend = diesel::sqlite::Sqlite;
/// The timestamp type, named once.
pub type Timestamp = chrono::NaiveDateTime;

diesel::table! {
    articles (id) {
        id -> Integer,
        title -> Text,
        body -> Text,
        created_at -> Timestamp,
        updated_at -> Timestamp,
        deleted_at -> Nullable<Timestamp>,
        version -> Integer,
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, Queryable, Selectable, Insertable, AsChangeset, toolbox_db::Entity,
)]
#[diesel(table_name = articles)]
#[diesel(check_for_backend(Backend))]
#[entity(
    backend = Backend,
    id = id,
    timestamps,
    soft_delete = deleted_at,
    version = version,
    sortable(id, title),
)]
pub struct Article {
    pub id: i32,
    pub title: String,
    pub body: String,
    pub created_at: Timestamp,
    pub updated_at: Timestamp,
    pub deleted_at: Option<Timestamp>,
    pub version: i32,
}

impl Article {
    fn new(id: i32, title: &str) -> Self {
        let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc();
        Self {
            id,
            title: title.to_owned(),
            body: "b".to_owned(),
            created_at: epoch,
            updated_at: epoch,
            deleted_at: None,
            version: 0,
        }
    }
}

const DDL: &str = "CREATE TABLE articles (
    id INTEGER PRIMARY KEY,
    title TEXT NOT NULL,
    body TEXT NOT NULL,
    created_at TIMESTAMP NOT NULL,
    updated_at TIMESTAMP NOT NULL,
    deleted_at TIMESTAMP,
    version INTEGER NOT NULL DEFAULT 0
);";

fn db() -> (Db<SqliteConnection>, tempfile::TempDir) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("t.sqlite3");
    let db = Db::<SqliteConnection>::builder(path.to_string_lossy().into_owned())
        .sqlite_pragmas(toolbox_db::SqlitePragmas::default())
        .build()
        .unwrap();
    db.blocking_conn().unwrap().batch_execute(DDL).unwrap();
    (db, dir)
}

#[tokio::test]
async fn save_inserts_then_updates() {
    let (db, _d) = db();
    let saved = db
        .run(|c: &mut SqliteConnection| Article::new(1, "first").save(c))
        .await
        .unwrap();
    assert_eq!(saved.title, "first");
    assert_eq!(
        saved.version, 0,
        "an insert stores the version it was built with"
    );

    let again = db
        .run(move |c: &mut SqliteConnection| {
            let mut a = Article::find_by_id(c, &1)?.unwrap();
            a.title = "second".to_owned();
            a.save(c)
        })
        .await
        .unwrap();
    assert_eq!(again.title, "second");
    assert_eq!(again.version, 1, "an update bumps the version");

    let n = db
        .run(|c: &mut SqliteConnection| Article::count(c))
        .await
        .unwrap();
    assert_eq!(n, 1, "an update is not a second row");
}

#[tokio::test]
async fn timestamps_are_populated_on_insert_and_touched_on_update() {
    let (db, _d) = db();
    let epoch = chrono::DateTime::from_timestamp(0, 0).unwrap().naive_utc();

    let saved = db
        .run(|c: &mut SqliteConnection| Article::new(1, "x").save(c))
        .await
        .unwrap();
    assert!(saved.created_at > epoch, "created_at was set on insert");
    assert!(saved.updated_at > epoch, "updated_at was set on insert");

    let created = saved.created_at;
    let updated = db
        .run(move |c: &mut SqliteConnection| {
            let mut a = Article::find_by_id(c, &1)?.unwrap();
            a.body = "changed".to_owned();
            a.save(c)
        })
        .await
        .unwrap();
    assert_eq!(updated.created_at, created, "created_at survives an update");
    assert!(updated.updated_at >= created);
}

#[tokio::test]
async fn a_stale_version_is_a_conflict_rather_than_a_lost_update() {
    let (db, _d) = db();
    db.run(|c: &mut SqliteConnection| Article::new(1, "x").save(c))
        .await
        .unwrap();

    let result = db
        .run(move |c: &mut SqliteConnection| {
            let mut a = Article::find_by_id(c, &1)?.unwrap();
            let mut stale = a.clone();
            a.title = "winner".to_owned();
            a.save(c)?;
            // `stale` still carries the version it was read at.
            stale.title = "loser".to_owned();
            stale.save(c)
        })
        .await;

    assert!(matches!(result, Err(DbError::Conflict)), "{result:?}");

    let title = db
        .run(|c: &mut SqliteConnection| {
            Ok::<_, DbError>(Article::find_by_id(c, &1)?.unwrap().title)
        })
        .await
        .unwrap();
    assert_eq!(title, "winner", "the first writer kept the row");
}

#[tokio::test]
async fn soft_delete_hides_the_row_from_every_read() {
    let (db, _d) = db();
    db.run(|c: &mut SqliteConnection| {
        Article::new(1, "a").save(c)?;
        Article::new(2, "b").save(c)
    })
    .await
    .unwrap();

    let n = db
        .run(|c: &mut SqliteConnection| Article::delete_by_id(c, &1))
        .await
        .unwrap();
    assert_eq!(n, 1);

    let found = db
        .run(|c: &mut SqliteConnection| Article::find_by_id(c, &1))
        .await
        .unwrap();
    assert!(found.is_none(), "find_by_id skips a soft-deleted row");
    assert!(
        !db.run(|c: &mut SqliteConnection| Article::exists(c, &1))
            .await
            .unwrap()
    );
    assert_eq!(
        db.run(|c: &mut SqliteConnection| Article::count(c))
            .await
            .unwrap(),
        1
    );

    let ids = db
        .run(|c: &mut SqliteConnection| Article::find_by_ids(c, &[1, 2]))
        .await
        .unwrap();
    assert_eq!(ids.len(), 1);

    // The row is still there; it is marked, not gone.
    let raw: i64 = db
        .query(|c: &mut SqliteConnection| articles::table.count().get_result(c))
        .await
        .unwrap();
    assert_eq!(raw, 2);
}

#[tokio::test]
async fn deleting_an_already_deleted_row_changes_nothing() {
    let (db, _d) = db();
    db.run(|c: &mut SqliteConnection| Article::new(1, "a").save(c))
        .await
        .unwrap();
    assert_eq!(
        db.run(|c: &mut SqliteConnection| Article::delete_by_id(c, &1))
            .await
            .unwrap(),
        1
    );
    assert_eq!(
        db.run(|c: &mut SqliteConnection| Article::delete_by_id(c, &1))
            .await
            .unwrap(),
        0
    );
}

#[tokio::test]
async fn page_orders_by_the_declared_fields_only() {
    let (db, _d) = db();
    db.run(|c: &mut SqliteConnection| {
        for i in 1..=7 {
            Article::new(i, &format!("t{i}")).save(c)?;
        }
        Ok::<_, DbError>(())
    })
    .await
    .unwrap();

    let request = PageRequest::paged(0, 3, Sort::parse("-id").unwrap()).unwrap();
    let page = db
        .run(move |c: &mut SqliteConnection| Article::page(c, &request))
        .await
        .unwrap();
    assert_eq!(page.len(), 3);
    assert_eq!(page.total(), 7);
    assert_eq!(page.items()[0].id, 7);

    let bad = PageRequest::paged(0, 3, Sort::parse("body").unwrap()).unwrap();
    let err = db
        .run(move |c: &mut SqliteConnection| Article::page(c, &bad))
        .await;
    assert!(
        matches!(err, Err(DbError::InvalidSortField { .. })),
        "{err:?}"
    );
}

#[tokio::test]
async fn page_past_the_end_is_empty_but_still_reports_the_real_total() {
    let (db, _d) = db();
    db.run(|c: &mut SqliteConnection| {
        for i in 1..=7 {
            Article::new(i, &format!("t{i}")).save(c)?;
        }
        Ok::<_, DbError>(())
    })
    .await
    .unwrap();

    let request = PageRequest::paged(1000, 10, Sort::unsorted()).unwrap();
    let page = db
        .run(move |c: &mut SqliteConnection| Article::page(c, &request))
        .await
        .unwrap();
    assert!(page.is_empty());
    assert_eq!(
        page.total(),
        7,
        "the window count had no row to ride on, so `page` fell back to `count`"
    );
    assert_eq!(page.total_pages(), Some(1));
}

#[test]
fn the_sortable_allowlist_is_exactly_what_was_declared() {
    assert_eq!(Article::sortable_fields(), ["id", "title"]);
}

#[tokio::test]
async fn save_all_is_atomic() {
    let (db, _d) = db();
    let saved = db
        .run(|c: &mut SqliteConnection| {
            Article::save_all(c, &[Article::new(1, "a"), Article::new(2, "b")])
        })
        .await
        .unwrap();
    assert_eq!(saved.len(), 2);
    assert_eq!(
        db.run(|c: &mut SqliteConnection| Article::count(c))
            .await
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn delete_by_ids_soft_deletes_each_one() {
    let (db, _d) = db();
    db.run(|c: &mut SqliteConnection| {
        Article::save_all(
            c,
            &[
                Article::new(1, "a"),
                Article::new(2, "b"),
                Article::new(3, "c"),
            ],
        )
    })
    .await
    .unwrap();

    let n = db
        .run(|c: &mut SqliteConnection| Article::delete_by_ids(c, &[1, 3]))
        .await
        .unwrap();
    assert_eq!(n, 2);
    assert_eq!(
        db.run(|c: &mut SqliteConnection| Article::count(c))
            .await
            .unwrap(),
        1
    );
}

#[tokio::test]
async fn truncate_removes_everything_including_soft_deleted_rows() {
    let (db, _d) = db();
    db.run(|c: &mut SqliteConnection| {
        Article::save_all(c, &[Article::new(1, "a"), Article::new(2, "b")])?;
        Article::delete_by_id(c, &1)
    })
    .await
    .unwrap();

    let n = db
        .run(|c: &mut SqliteConnection| Article::truncate(c))
        .await
        .unwrap();
    assert_eq!(n, 2);
}

/// The escape hatch is the design's safety valve: anything the derive does not
/// generate is one `query()` away, and pagination still composes onto it.
#[tokio::test]
async fn query_composes_with_a_hand_written_filter_and_still_paginates() {
    use toolbox_db::Paginate as _;

    let (db, _d) = db();
    db.run(|c: &mut SqliteConnection| {
        let items: Vec<Article> = (1..=10)
            .map(|i| Article::new(i, &format!("t{i}")))
            .collect();
        Article::save_all(c, &items)
    })
    .await
    .unwrap();

    let request = PageRequest::paged(0, 2, Sort::unsorted()).unwrap();
    let page = db
        .run(move |c: &mut SqliteConnection| {
            Article::query()
                .filter(articles::id.gt(5))
                .select(Article::as_select())
                .paginate(&request)
                .load_page::<Article, _>(c)
        })
        .await
        .unwrap();

    assert_eq!(page.len(), 2);
    assert_eq!(
        page.total(),
        5,
        "the total respects the hand-written filter"
    );
}

#[tokio::test]
async fn the_entity_trait_names_the_id() {
    let (db, _d) = db();
    let saved = db
        .run(|c: &mut SqliteConnection| Article::new(42, "x").save(c))
        .await
        .unwrap();
    assert_eq!(saved.id(), Some(&42));
}

// --- The autoincrement flavour, whose insert has to read the id back ------

diesel::table! {
    notes (id) {
        id -> Integer,
        text -> Text,
    }
}

#[derive(
    Debug, Clone, PartialEq, Eq, Queryable, Selectable, Insertable, AsChangeset, toolbox_db::Entity,
)]
#[diesel(table_name = notes)]
#[diesel(check_for_backend(Backend))]
#[entity(backend = Backend, id = id, autoincrement, sortable(id))]
pub struct Note {
    #[diesel(skip_insertion)]
    pub id: i32,
    pub text: String,
}

const NOTES_DDL: &str = "CREATE TABLE notes (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    text TEXT NOT NULL
);";

fn notes_db() -> (Db<SqliteConnection>, tempfile::TempDir) {
    let (db, dir) = db();
    db.blocking_conn()
        .unwrap()
        .batch_execute(NOTES_DDL)
        .unwrap();
    (db, dir)
}

#[tokio::test]
async fn an_autoincrement_insert_reads_the_generated_id_back() {
    let (db, _d) = notes_db();
    let saved = db
        .run(|c: &mut SqliteConnection| {
            Note {
                id: 0,
                text: "first".to_owned(),
            }
            .save(c)
        })
        .await
        .unwrap();
    assert!(saved.id > 0, "the database assigned an id: {}", saved.id);
    assert_eq!(saved.text, "first");

    let second = db
        .run(|c: &mut SqliteConnection| {
            Note {
                id: 0,
                text: "second".to_owned(),
            }
            .save(c)
        })
        .await
        .unwrap();
    assert!(second.id > saved.id);
    assert_eq!(
        db.run(|c: &mut SqliteConnection| Note::count(c))
            .await
            .unwrap(),
        2
    );
}

#[tokio::test]
async fn saving_an_autoincrement_row_that_has_an_id_updates_it() {
    let (db, _d) = notes_db();
    let saved = db
        .run(|c: &mut SqliteConnection| {
            Note {
                id: 0,
                text: "a".to_owned(),
            }
            .save(c)
        })
        .await
        .unwrap();
    let id = saved.id;

    let updated = db
        .run(move |c: &mut SqliteConnection| {
            let mut n = Note::find_by_id(c, &id)?.unwrap();
            n.text = "b".to_owned();
            n.save(c)
        })
        .await
        .unwrap();

    assert_eq!(updated.id, id, "an update keeps the id");
    assert_eq!(updated.text, "b");
    assert_eq!(
        db.run(|c: &mut SqliteConnection| Note::count(c))
            .await
            .unwrap(),
        1
    );
}

/// Zero is the "not inserted yet" sentinel for an autoincrement key, which is
/// what lets one method serve both insert and update.
#[test]
fn a_zero_id_reads_as_not_yet_inserted() {
    assert_eq!(
        Note {
            id: 0,
            text: String::new()
        }
        .id(),
        None
    );
    assert_eq!(
        Note {
            id: 7,
            text: String::new()
        }
        .id(),
        Some(&7)
    );
}

#[tokio::test]
async fn an_entity_without_soft_delete_really_deletes() {
    let (db, _d) = notes_db();
    let saved = db
        .run(|c: &mut SqliteConnection| {
            Note {
                id: 0,
                text: "a".to_owned(),
            }
            .save(c)
        })
        .await
        .unwrap();
    let n = db
        .run(move |c: &mut SqliteConnection| Note::delete_by_id(c, &saved.id))
        .await
        .unwrap();
    assert_eq!(n, 1);
    assert_eq!(
        db.run(|c: &mut SqliteConnection| Note::count(c))
            .await
            .unwrap(),
        0
    );
}

/// `save` is a read-modify-write - decide whether the row exists, then write
/// it - so it runs in a transaction. Nested inside a caller's transaction it
/// becomes a savepoint, which is what makes `save_all` atomic.
#[tokio::test]
async fn a_save_inside_a_rolled_back_transaction_leaves_nothing() {
    let (db, _d) = db();
    let result: Result<(), DbError> = db
        .transaction(|c: &mut SqliteConnection| {
            Article::new(1, "written").save(c)?;
            // Whatever came next failed.
            Err(DbError::Conflict)
        })
        .await;
    assert!(result.is_err());

    assert_eq!(
        db.run(|c: &mut SqliteConnection| Article::count(c))
            .await
            .unwrap(),
        0,
        "the save rolled back with the transaction that contained it"
    );
}

#[tokio::test]
async fn save_all_rolls_back_every_row_when_one_fails() {
    let (db, _d) = db();
    // Two rows sharing an id: the second is an update of the first, so this
    // succeeds; then a version conflict aborts the batch.
    let result = db
        .run(|c: &mut SqliteConnection| {
            Article::save_all(c, &[Article::new(1, "a"), Article::new(2, "b")])?;
            let mut stale = Article::find_by_id(c, &1)?.unwrap();
            stale.version = 99; // a version nobody has
            stale.save(c)
        })
        .await;

    assert!(matches!(result, Err(DbError::Conflict)), "{result:?}");
    // The first batch committed; only the conflicting save was undone.
    assert_eq!(
        db.run(|c: &mut SqliteConnection| Article::count(c))
            .await
            .unwrap(),
        2
    );
}
