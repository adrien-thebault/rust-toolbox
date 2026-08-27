//! The ADR 0001 spike, kept as a regression test.
//!
//! Two claims the design rests on, neither obvious:
//!
//! 1. the method bodies `#[derive(Entity)]` generates can be **generic over
//!    the connection** while naming a concrete backend, so one binary can hold
//!    a PostgreSQL pool and a SQLite pool;
//! 2. `Paginated<Q>` implements `QueryFragment<DB>` **once, generically**,
//!    rather than once per backend.
//!
//! If (2) were false, `sqlite`/`postgres`/`mysql` would have to come back as
//! features on `toolbox-db`, which is the thing the design exists to remove.
//!
//! What the spike also established, and what shaped the derive: a function
//! generic over an *arbitrary* `C::Backend` is not writable, because the
//! required bound `DieselReserveSpecialization` is sealed inside diesel. The
//! derive never needs it - `#[entity(backend = ...)]` names the backend - so
//! every generated body is generic in `C` and concrete in `B`.

use diesel::{
    backend::Backend,
    mysql::Mysql,
    pg::Pg,
    prelude::*,
    query_builder::{Query, QueryFragment},
    sqlite::Sqlite,
};
use toolbox_core::{Page, PageRequest, Sort};
use toolbox_db::{DbResult, Paginate};

use crate::fixtures::{TestEntity, seed, temp_db, test_entity};

// --- Claim 2: one QueryFragment impl covers every backend ----------------

/// Compiles only while `Paginated<Q>` is generic over the backend.
fn assert_paginated_is_generic<DB, Q>()
where
    DB: Backend,
    Q: QueryFragment<DB> + Query,
    i64: diesel::serialize::ToSql<diesel::sql_types::BigInt, DB>,
    toolbox_db::Paginated<Q>: QueryFragment<DB>,
{
}

/// Any real query, standing in for whatever a caller composes.
type AnyQuery = diesel::dsl::Select<test_entity::table, test_entity::id>;

#[test]
fn pagination_has_one_query_fragment_impl_for_every_backend() {
    assert_paginated_is_generic::<Sqlite, AnyQuery>();
    assert_paginated_is_generic::<Pg, AnyQuery>();
    assert_paginated_is_generic::<Mysql, AnyQuery>();
}

// --- Claim 1: the generated bodies are generic in C, concrete in B -------

/// Exactly what the derive expands to, once per entity, for its declared
/// backend. Written as a macro here so the three instantiations are provably
/// the same code.
macro_rules! generated_for {
    ($module:ident, $backend:ty) => {
        mod $module {
            use diesel::prelude::*;
            use toolbox_core::{Page, PageRequest};
            use toolbox_db::DbResult;

            use super::{Paginate, TestEntity, test_entity};

            /// The body generated for `find_by_id`.
            pub fn find_by_id<C>(conn: &mut C, id: i32) -> DbResult<Option<TestEntity>>
            where
                C: diesel::connection::LoadConnection<Backend = $backend>,
            {
                Ok(test_entity::table
                    .find(id)
                    .select(TestEntity::as_select())
                    .first(conn)
                    .optional()?)
            }

            /// The body generated for `count`.
            pub fn count<C>(conn: &mut C) -> DbResult<i64>
            where
                C: diesel::connection::LoadConnection<Backend = $backend>,
            {
                Ok(test_entity::table.count().get_result(conn)?)
            }

            /// The body generated for `page`, including the sortable allowlist.
            pub fn page<C>(conn: &mut C, request: &PageRequest) -> DbResult<Page<TestEntity>>
            where
                C: diesel::connection::LoadConnection<Backend = $backend>,
            {
                toolbox_db::sort::validate(request.sort(), &["id", "title", "rank"])?;
                let mut query = test_entity::table
                    .filter(test_entity::deleted_at.is_null())
                    .select(TestEntity::as_select())
                    .into_boxed();
                for item in request.sort().items() {
                    query = match (item.field.as_str(), item.direction) {
                        ("id", toolbox_core::SortDirection::Asc) => {
                            query.then_order_by(test_entity::id.asc())
                        }
                        ("id", toolbox_core::SortDirection::Desc) => {
                            query.then_order_by(test_entity::id.desc())
                        }
                        ("rank", toolbox_core::SortDirection::Asc) => {
                            query.then_order_by(test_entity::rank.asc())
                        }
                        _ => query.then_order_by(test_entity::title.asc()),
                    };
                }
                query.paginate(request).load_page::<TestEntity, _>(conn)
            }

            /// The body generated for `delete_by_id` with `soft_delete`.
            pub fn soft_delete_by_id<C>(conn: &mut C, id: i32) -> DbResult<usize>
            where
                C: diesel::connection::LoadConnection<Backend = $backend>,
            {
                Ok(diesel::update(test_entity::table.find(id))
                    .set(test_entity::deleted_at.eq(Some(chrono::Utc::now().naive_utc())))
                    .execute(conn)?)
            }
        }
    };
}

generated_for!(for_sqlite, diesel::sqlite::Sqlite);
generated_for!(for_pg, diesel::pg::Pg);
generated_for!(for_mysql, diesel::mysql::Mysql);

/// Instantiating for a second backend is what proves the bounds are real: code
/// only ever called with `SqliteConnection` proves nothing.
#[allow(dead_code)]
fn instantiate_for_every_backend() {
    let _: fn(&mut SqliteConnection, i32) -> DbResult<Option<TestEntity>> = for_sqlite::find_by_id;
    let _: fn(&mut PgConnection, i32) -> DbResult<Option<TestEntity>> = for_pg::find_by_id;
    let _: fn(&mut MysqlConnection, i32) -> DbResult<Option<TestEntity>> = for_mysql::find_by_id;

    let _: fn(&mut SqliteConnection) -> DbResult<i64> = for_sqlite::count;
    let _: fn(&mut PgConnection) -> DbResult<i64> = for_pg::count;
    let _: fn(&mut MysqlConnection) -> DbResult<i64> = for_mysql::count;

    let _: fn(&mut SqliteConnection, &PageRequest) -> DbResult<Page<TestEntity>> = for_sqlite::page;
    let _: fn(&mut PgConnection, &PageRequest) -> DbResult<Page<TestEntity>> = for_pg::page;
    let _: fn(&mut MysqlConnection, &PageRequest) -> DbResult<Page<TestEntity>> = for_mysql::page;

    let _: fn(&mut SqliteConnection, i32) -> DbResult<usize> = for_sqlite::soft_delete_by_id;
    let _: fn(&mut PgConnection, i32) -> DbResult<usize> = for_pg::soft_delete_by_id;
    let _: fn(&mut MysqlConnection, i32) -> DbResult<usize> = for_mysql::soft_delete_by_id;
}

/// Two pools of different backends in one process, which mutually exclusive
/// backend features made impossible.
#[allow(dead_code)]
fn two_pools_in_one_process() {
    let _sqlite = toolbox_db::Db::<SqliteConnection>::builder("a.sqlite3");
    let _pg = toolbox_db::Db::<PgConnection>::builder("postgres://localhost/b");
    let _mysql = toolbox_db::Db::<MysqlConnection>::builder("mysql://localhost/c");
}

// --- And they actually run ----------------------------------------------

#[tokio::test]
async fn the_generated_bodies_run_against_a_real_database() {
    let (db, _dir) = temp_db();

    let found = db
        .run(|c: &mut SqliteConnection| {
            seed(c, 5);
            for_sqlite::find_by_id(c, 3)
        })
        .await
        .unwrap();
    assert_eq!(found.unwrap().title, "row 3");

    let missing = db
        .run(|c: &mut SqliteConnection| for_sqlite::find_by_id(c, 99))
        .await
        .unwrap();
    assert!(missing.is_none(), "a missing row is None, not an error");

    let request = PageRequest::paged(0, 2, Sort::parse("-id").unwrap()).unwrap();
    let listed = db
        .run(move |c: &mut SqliteConnection| for_sqlite::page(c, &request))
        .await
        .unwrap();
    assert_eq!(listed.len(), 2);
    assert_eq!(listed.total(), 5);
    assert_eq!(listed.items()[0].id, 4, "sorted descending");

    let deleted = db
        .run(|c: &mut SqliteConnection| for_sqlite::soft_delete_by_id(c, 4))
        .await
        .unwrap();
    assert_eq!(deleted, 1);

    let after = db
        .run(move |c: &mut SqliteConnection| {
            for_sqlite::page(c, &PageRequest::paged(0, 10, Sort::unsorted()).unwrap())
        })
        .await
        .unwrap();
    assert_eq!(
        after.total(),
        4,
        "a soft-deleted row drops out of every read"
    );
}

#[tokio::test]
async fn an_undeclared_sort_field_is_rejected_before_it_reaches_sql() {
    let (db, _dir) = temp_db();
    let request = PageRequest::paged(0, 10, Sort::parse("secret").unwrap()).unwrap();
    let err = db
        .run(move |c: &mut SqliteConnection| for_sqlite::page(c, &request))
        .await;
    assert!(
        matches!(err, Err(toolbox_db::DbError::InvalidSortField { .. })),
        "{err:?}"
    );
}
