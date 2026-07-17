use crate::diesel_tools::{
    database::{DatabasePooledConnection, DatabaseResult},
    repository::Repository,
};
use diesel::Connection;

/// persists (create or update) entities. No `AsChangeset` bound here: the
/// trait's own signatures don't need it (only the `impl_save!`-generated
/// bodies do, on their concrete entity types), and leaving it off keeps
/// read-only repositories from having to derive it.
pub trait Save: Repository {
    /// persists one entity
    fn save(
        db: &mut DatabasePooledConnection,
        entity: &Self::Entity,
    ) -> DatabaseResult<Self::Entity>;

    /// persists multiple entities atomically: one failure rolls the whole
    /// batch back, so a caller never observes a partial save
    fn save_all(
        db: &mut DatabasePooledConnection,
        entities: &[Self::Entity],
    ) -> DatabaseResult<Vec<Self::Entity>> {
        db.transaction(|db| {
            entities
                .iter()
                .map(|entity| Self::save(db, entity))
                .collect()
        })
    }
}

/// implements [`Save`] for the given repository.
///
/// Two flavors:
/// - `impl_save!($repo)`: the caller always supplies a known id (a natural
///   key, or a fixed/singleton one) - plain upsert-by-id.
/// - `impl_save!($repo, autoincrement)`: the entity's `id` field is
///   `Option<Self::Id>`. `None` lets the database assign an id
///   (`AUTOINCREMENT`/`SERIAL`); `Some` upserts by that id. Assumes the
///   field is named `id`, and (for the mysql branch, which has no
///   `RETURNING`) that it's an autoincrement `Integer` (`i32`).
///
/// Postgres and SQLite both support `RETURNING` on an upsert and share one
/// macro body (SQLite needs the `returning_clauses_for_sqlite_3_35` diesel
/// feature, SQLite ≥ 3.35). MySQL has no `RETURNING`, so it falls back to
/// insert-then-look-up via `LAST_INSERT_ID()`.
#[cfg(any(feature = "postgresql", feature = "sqlite"))]
#[macro_export]
macro_rules! impl_save {
    // one arm for both flavors: with RETURNING, "the caller supplies the id"
    // and "the database may assign it" generate the same statement - a fresh
    // row (id absent -> DEFAULT, never conflicts) or an upserted one, either
    // way `get_result` reads back the final row in one statement.
    ($repo:ty $(, autoincrement)?) => {
        const _: () = {
            use diesel::prelude::*;
            use $crate::diesel_tools::{
                DatabasePooledConnection, DatabaseResult, Repository, Save,
            };

            #[allow(non_local_definitions)]
            impl Save for $repo {
                fn save(
                    db: &mut DatabasePooledConnection,
                    entity: &Self::Entity,
                ) -> DatabaseResult<Self::Entity> {
                    Ok(diesel::insert_into(Self::table())
                        .values(entity)
                        .on_conflict(Table::primary_key(&Self::table()))
                        .do_update()
                        .set(entity)
                        .get_result(db)?)
                }
            }
        };
    };
}

/// implements [`Save`] for the given repository (MySQL: no `RETURNING`, so
/// this falls back to insert-then-look-up via `LAST_INSERT_ID()`). See the
/// `postgresql`/`sqlite` definition of this macro for the full contract.
#[cfg(feature = "mysql")]
#[macro_export]
macro_rules! impl_save {
    ($repo:ty) => {
        const _: () = {
            use diesel::prelude::*;
            use $crate::diesel_tools::{
                DatabasePooledConnection, DatabaseResult, Repository, Save,
            };

            #[allow(non_local_definitions)]
            impl Save for $repo {
                fn save(
                    db: &mut DatabasePooledConnection,
                    entity: &Self::Entity,
                ) -> DatabaseResult<Self::Entity> {
                    diesel::insert_into(Self::table())
                        .values(entity)
                        .on_conflict(diesel::dsl::DuplicatedKeys)
                        .do_update()
                        .set(entity)
                        .execute(db)?;

                    Ok(Self::table().find(entity.id()).get_result(db)?)
                }
            }
        };
    };

    ($repo:ty, autoincrement) => {
        const _: () = {
            use diesel::prelude::*;
            use $crate::diesel_tools::{
                DatabasePooledConnection, DatabaseResult, Repository, Save,
            };

            #[allow(non_local_definitions)]
            impl Save for $repo {
                // mysql has no RETURNING: insert/upsert, then look the row up
                // by the id we already knew, or by LAST_INSERT_ID() if we
                // let the database assign it.
                fn save(
                    db: &mut DatabasePooledConnection,
                    entity: &Self::Entity,
                ) -> DatabaseResult<Self::Entity> {
                    use diesel::{dsl::sql, sql_types::BigInt};

                    Ok(db.transaction(|conn| {
                        diesel::insert_into(Self::table())
                            .values(entity)
                            .on_conflict(diesel::dsl::DuplicatedKeys)
                            .do_update()
                            .set(entity)
                            .execute(conn)?;

                        let id = match entity.id {
                            Some(id) => id,
                            None => {
                                // LAST_INSERT_ID() is BIGINT UNSIGNED on the
                                // wire - select it as BigInt (lying to diesel
                                // about the SQL type risks a deserialization
                                // error) and narrow to the entity's i32 id.
                                let id: i64 = diesel::select(sql::<BigInt>("LAST_INSERT_ID()"))
                                    .get_result(conn)?;
                                i32::try_from(id).map_err(|e| {
                                    diesel::result::Error::DeserializationError(Box::new(e))
                                })?
                            }
                        };

                        Self::table().find(id).get_result(conn)
                    })?)
                }
            }
        };
    };
}

#[cfg(test)]
mod tests {
    use crate::diesel_tools::repository::Repository;
    use diesel::prelude::*;

    diesel::table! {
        test_entity {
            id -> Integer,
            field_a -> Text,
            field_b -> Text,
        }
    }

    #[derive(
        Clone, Queryable, Selectable, Identifiable, PartialEq, Debug, Insertable, Eq, AsChangeset,
    )]
    #[diesel(table_name = test_entity)]
    #[diesel(primary_key(id))]
    struct TestEntity {
        pub id: i32,
        pub field_a: String,
        pub field_b: String,
    }

    struct TestRepository;

    impl Repository for TestRepository {
        type Entity = TestEntity;
        type Id = i32;
        type IdColumn = test_entity::id;
        type Table = test_entity::table;
    }

    /// ensures that the impl_save macro produces valid code
    #[test]
    fn test_impl_save() {
        impl_save!(TestRepository);
        let _ = TestRepository {};
    }

    diesel::table! {
        test_auto_entity {
            id -> Integer,
            field_a -> Text,
        }
    }

    #[derive(Clone, Queryable, Selectable, PartialEq, Debug, Insertable, Eq, AsChangeset)]
    #[diesel(table_name = test_auto_entity)]
    struct TestAutoEntity {
        #[diesel(deserialize_as = i32)]
        pub id: Option<i32>,
        pub field_a: String,
    }

    struct TestAutoRepository;

    impl Repository for TestAutoRepository {
        type Entity = TestAutoEntity;
        type Id = i32;
        type IdColumn = test_auto_entity::id;
        type Table = test_auto_entity::table;
    }

    /// ensures that the impl_save!($repo, autoincrement) arm produces valid code
    #[test]
    fn test_impl_save_autoincrement() {
        impl_save!(TestAutoRepository, autoincrement);
        let _ = TestAutoRepository {};
    }
}
