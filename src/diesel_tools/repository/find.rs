use crate::diesel_tools::{
    database::{DatabasePooledConnection, DatabaseResult},
    pagination::{Page, PageRequest},
    repository::Repository,
};
use std::slice;

/// retrieves entities
pub trait Find: Repository {
    /// counts the number of entities matching the given filters
    fn count(db: &mut DatabasePooledConnection) -> DatabaseResult<i64>;

    /// checks the existence of an entity using its id
    fn exists_by_id(db: &mut DatabasePooledConnection, id: &Self::Id) -> DatabaseResult<bool> {
        Self::exists_by_id_in(db, slice::from_ref(id)).map(|r| {
            r.into_iter()
                .next()
                .map(|(_, exists)| exists)
                .unwrap_or_default()
        })
    }

    /// checks the existence of entities using their id
    fn exists_by_id_in(
        db: &mut DatabasePooledConnection,
        ids: &[Self::Id],
    ) -> DatabaseResult<Vec<(Self::Id, bool)>>;

    /// retrieves one entity using its id
    fn find_by_id(
        db: &mut DatabasePooledConnection,
        id: &Self::Id,
    ) -> DatabaseResult<Option<Self::Entity>> {
        Self::find_by_id_in(db, slice::from_ref(id)).map(|r| r.into_iter().next())
    }

    /// retrieves all the entities using their id
    fn find_by_id_in(
        db: &mut DatabasePooledConnection,
        ids: &[Self::Id],
    ) -> DatabaseResult<Vec<Self::Entity>>;

    /// retrieves all the entities, paginated
    fn find_all(
        db: &mut DatabasePooledConnection,
        page_request: PageRequest,
    ) -> DatabaseResult<Page<Self::Entity>>;
}

/// implements find for the given repository
#[macro_export]
macro_rules! impl_find {
    ($repo:ty) => {
        const _: () = {
            use diesel::{associations::HasTable, prelude::*};
            use std::collections::HashSet;
            use $crate::diesel_tools::{
                DatabasePooledConnection, DatabaseResult, Find, Page, PageRequest, Repository,
            };

            #[allow(non_local_definitions)]
            impl Find for $repo {
                fn count(db: &mut DatabasePooledConnection) -> DatabaseResult<i64> {
                    Ok(Self::Table::table()
                        .select(diesel::dsl::count_star())
                        .first(db)?)
                }

                fn exists_by_id_in(
                    db: &mut DatabasePooledConnection,
                    ids: &[Self::Id],
                ) -> DatabaseResult<Vec<(Self::Id, bool)>> {
                    let existing = Self::Table::table()
                        .select(Self::id_column())
                        .filter(Self::id_column().eq_any(ids))
                        .get_results(db)?
                        .into_iter()
                        .collect::<HashSet<Self::Id>>();

                    Ok(ids
                        .into_iter()
                        .map(|id| (id.clone(), existing.contains(id)))
                        .collect())
                }

                fn find_by_id_in(
                    db: &mut DatabasePooledConnection,
                    ids: &[Self::Id],
                ) -> DatabaseResult<Vec<Self::Entity>> {
                    Ok(Self::Table::table()
                        .filter(Self::id_column().eq_any(ids))
                        .get_results::<Self::Entity>(db)?)
                }

                fn find_all(
                    db: &mut DatabasePooledConnection,
                    page_request: PageRequest,
                ) -> DatabaseResult<Page<Self::Entity>> {
                    let data =
                        Self::apply_page_request(Self::Table::table().into_boxed(), &page_request)
                            .get_results::<Self::Entity>(db)?;
                    let total_elements = Self::count(db)?;

                    Ok(Page {
                        data,
                        page_request,
                        total_elements,
                    })
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

    /// ensures that the impl_find macro produces valid code
    #[test]
    fn test_impl_find() {
        impl_find!(TestRepository);
        let _ = TestRepository {};
    }
}
