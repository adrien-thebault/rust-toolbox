use crate::diesel_tools::{database::Database, pagination::PageRequest, sort::Sort};
use diesel::{
    Column,
    associations::HasTable,
    dsl::IntoBoxed,
    query_dsl::methods::{BoxedDsl, LimitDsl, OffsetDsl},
};
use tracing::warn;

/// mod for the find trait and its implementation
#[macro_use]
mod find;
pub use find::*;

/// mod for the save trait and its implementation
#[macro_use]
mod save;
pub use save::*;

/// mod for the delete trait and its implementation
#[macro_use]
mod delete;
pub use delete::*;

/// represents a repository for a given entity type
pub trait Repository {
    /// the diesel entity this repository reads and writes. Deliberately
    /// unbounded here - only [`Save`](crate::diesel_tools::Save) needs
    /// `AsChangeset`, so the write-side bound lives there and a read-only
    /// repository's entity doesn't have to derive it.
    type Entity;
    /// the type used to look entities up (passed to `find_by_id`/`delete_by_id`/etc.)
    type Id;
    /// the diesel column type of the table's id column
    type IdColumn: Column + Default;
    /// the diesel table this repository is backed by
    type Table: HasTable;

    /// returns the table for the repository
    fn table() -> <Self::Table as HasTable>::Table {
        Self::Table::table()
    }

    /// returns the id column for the repository
    fn id_column() -> Self::IdColumn {
        Self::IdColumn::default()
    }

    /// applies the page_request's offset/limit/sort to the query.
    ///
    /// `offset`/`size` are defensively clamped to >= 0: SQLite treats a
    /// negative `LIMIT` as "no limit", so an unvalidated negative size from
    /// an untrusted caller would otherwise return the whole table. Use
    /// [`PageRequest::paged`] to reject such values up front instead of
    /// getting the clamped (empty) page.
    fn apply_page_request<'a>(
        mut query: IntoBoxed<'a, Self::Table, Database>,
        page_request: &PageRequest,
    ) -> IntoBoxed<'a, Self::Table, Database>
    where
        <Self as Repository>::Table: BoxedDsl<'a, Database>,
        <<Self as Repository>::Table as BoxedDsl<'a, Database>>::Output: LimitDsl<Output = IntoBoxed<'a, Self::Table, Database>>
            + OffsetDsl<Output = IntoBoxed<'a, Self::Table, Database>>,
    {
        let sort = match page_request {
            PageRequest::Unpaged { sort } => sort,
            PageRequest::Paged { offset, size, sort } => {
                query = query.offset((*offset).max(0)).limit((*size).max(0));
                sort
            }
        };

        Self::apply_sort(query, sort)
    }

    /// applies the sort to the query, if any. The default implementation
    /// warns and ignores it; `impl_repository!`'s `SortColumns` generates a
    /// real override per-repository.
    fn apply_sort<'a>(
        query: IntoBoxed<'a, Self::Table, Database>,
        _sort: &Sort,
    ) -> IntoBoxed<'a, Self::Table, Database>
    where
        <Self as Repository>::Table: BoxedDsl<'a, Database>,
    {
        if let Sort::Sorted { .. } = _sort {
            warn!("sort requested but not implemented for this repository");
        }

        query
    }
}

/// implements [`Repository`] and, via nested `impl_find!`/`impl_delete!`/
/// `impl_save!` calls, [`Find`](crate::diesel_tools::Find),
/// [`Delete`](crate::diesel_tools::Delete) and
/// [`Save`](crate::diesel_tools::Save) for the given entity type.
///
/// `Id` takes either shape:
/// - `(ty, id_column)` - the caller always supplies a known id (a natural
///   key, or a fixed/singleton one); backs a plain upsert-by-id `Save`.
/// - `(ty, id_column, autoincrement)` - the entity's `id` field is
///   `Option<ty>`; `Save` lets the database assign an id on `None`, upserts
///   by it on `Some`. See [`impl_save!`] for the full contract.
#[macro_export]
macro_rules! impl_repository {
    // explicit sort columns
    (
        $repo:ident {
            Schema = $($schema:ident)::+,
            Entity = $entity:ty,
            Id = ($id_ty:ty, $id_col:ident $(, $autoincrement:ident)?),
            SortColumns = { $($col:ident),* $(,)? } $(,)?
        }
    ) => {
        const _: () = {
            #[allow(unused_imports)]
            use $crate::diesel_tools::{Database, Repository, Sort, SortDirection};
            use diesel::{prelude::*, helper_types::IntoBoxed};
            use $($schema)::+ as __schema;

            #[allow(non_local_definitions)]
            impl Repository for $repo {
                type Entity = $entity;
                type Id = $id_ty;
                type IdColumn = __schema::$id_col;
                type Table = __schema::table;

                #[allow(unused_mut)]
                fn apply_sort<'a>(
                    mut query: IntoBoxed<'a, __schema::table, Database>,
                    sort: &Sort,
                ) -> IntoBoxed<'a, __schema::table, Database> {
                    if let Sort::Sorted { items } = sort {
                        for (field, direction) in items {
                            match direction {
                                $(
                                    SortDirection::Asc if field == <__schema::$col as Column>::NAME => {
                                        query = query.then_order_by(__schema::$col.asc());
                                    }
                                )*
                                $(
                                    SortDirection::Desc if field == <__schema::$col as Column>::NAME => {
                                        query = query.then_order_by(__schema::$col.desc());
                                    }
                                )*
                                _ => tracing::warn!("unknown sort field: {}", field),
                            }
                        }
                    }
                    query
                }
            }

            mod find_impl {
                $crate::impl_find!(super::$repo);
            }

            mod delete_impl {
                $crate::impl_delete!(super::$repo);
            }

            mod save_impl {
                $crate::impl_save!(super::$repo $(, $autoincrement)?);
            }
        };
    };

    // no sort columns: forward to the arm above with an empty list
    (
        $repo:ident {
            Schema = $($schema:ident)::+,
            Entity = $entity:ty,
            Id = ($id_ty:ty, $id_col:ident $(, $autoincrement:ident)?) $(,)?
        }
    ) => {
        impl_repository!($repo {
            Schema = $($schema)::+,
            Entity = $entity,
            Id = ($id_ty, $id_col $(, $autoincrement)?),
            SortColumns = {}
        });
    };
}

#[cfg(test)]
mod tests {
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

    /// tests that the impl_repository macro produces valid code
    #[test]
    fn test_impl_repository() {
        impl_repository!(TestRepository {
            Schema = test_entity,
            Entity = TestEntity,
            Id = (i32, id),
            SortColumns = { field_a, field_b },
        });

        let _ = TestRepository {};
    }
}
