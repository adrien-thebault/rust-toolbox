use crate::diesel_tools::{
    database::{DatabasePooledConnection, DatabaseResult},
    repository::Repository,
};
use std::slice;

/// delete one or more entity
pub trait Delete: Repository {
    /// deletes everything
    fn clear(db: &mut DatabasePooledConnection) -> DatabaseResult<usize>;

    /// deletes one entity by its id
    fn delete_by_id(db: &mut DatabasePooledConnection, id: &Self::Id) -> DatabaseResult<usize> {
        Self::delete_by_id_in(db, slice::from_ref(id))
    }

    /// deletes multiple entities using their id
    fn delete_by_id_in(
        db: &mut DatabasePooledConnection,
        ids: &[Self::Id],
    ) -> DatabaseResult<usize>;
}

/// implements delete for the given repository
#[macro_export]
macro_rules! impl_delete {
    ($repo:ty) => {
        const _: () = {
            use diesel::prelude::*;
            use $crate::diesel_tools::{
                DatabasePooledConnection, DatabaseResult, Delete, Repository,
            };

            #[allow(non_local_definitions)]
            impl Delete for $repo {
                /// deletes everything
                fn clear(db: &mut DatabasePooledConnection) -> DatabaseResult<usize> {
                    Ok(diesel::delete(Self::table()).execute(db)?)
                }

                /// deletes multiple entities using their id
                fn delete_by_id_in(
                    db: &mut DatabasePooledConnection,
                    ids: &[Self::Id],
                ) -> DatabaseResult<usize> {
                    Ok(
                        diesel::delete(Self::table().filter(Self::id_column().eq_any(ids)))
                            .execute(db)?,
                    )
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

    /// ensures that the impl_delete macro produces valid code
    #[test]
    fn test_impl_delete() {
        impl_delete!(TestRepository);
        let _ = TestRepository {};
    }
}
