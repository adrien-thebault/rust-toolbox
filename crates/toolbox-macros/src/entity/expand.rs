//! Generating the inherent CRUD methods.
//!
//! Every generated body is generic in the connection and concrete in the
//! backend. That is why `backend` is
//! a type path in the attribute: it lets one binary hold two pools, and it
//! keeps diesel's sealed `DieselReserveSpecialization` out of the where-clauses.

use proc_macro2::TokenStream;
use quote::quote;

use super::parse::{Dialect, EntityConfig};

// A `quote!` block is one expression however long the code it emits is, so the
// line-count lint measures the wrong thing here.

/// The whole `impl` block: the inherent methods a consumer calls.
///
/// # Arguments
///
/// * `cfg` - Everything the `#[entity(..)]` attribute declared, already
///   resolved against the struct.
pub fn expand(cfg: &EntityConfig) -> TokenStream {
    let EntityConfig {
        ident,
        table,
        backend,
        id_field,
        id_type,
        ..
    } = cfg;

    let alive = alive_filter(cfg);
    let sortable_names = cfg.sortable.iter().map(ToString::to_string);
    let sort_arms = sort_arms(cfg);
    let save_body = save_body(cfg);
    let delete_body = delete_body(cfg);
    let delete_many_body = delete_many_body(cfg);
    let id_accessor = id_accessor(cfg);

    quote! {
        #[automatically_derived]
        impl #ident {
            /// The escape hatch: a boxed query with the entity's own filters
            /// already applied, to compose your own `WHERE` onto.
            #[must_use]
            pub fn query<'__q>() -> #table::BoxedQuery<'__q, #backend> {
                use ::diesel::prelude::*;
                let __q = #table::table.into_boxed();
                #alive
                __q
            }

            /// The fields this entity may be sorted by.
            ///
            /// A sort naming anything else is rejected, never interpolated.
            #[must_use]
            pub const fn sortable_fields() -> &'static [&'static str] {
                &[#(#sortable_names),*]
            }

            /// Find one row by id.
            ///
            /// # Arguments
            ///
            /// * `conn` - The connection to run on. Generic in the
            ///   connection and concrete in the backend, so one binary can hold two
            ///   pools.
            /// * `id` - The primary key to look for. A miss is `Ok(None)`, not an
            ///   error.
            ///
            /// # Errors
            /// [`::toolbox_db::DbError::Query`] when the statement fails.
            pub fn find_by_id<__C>(
                conn: &mut __C,
                id: &#id_type,
            ) -> ::toolbox_db::DbResult<::core::option::Option<Self>>
            where
                __C: ::diesel::connection::LoadConnection<Backend = #backend>,
            {
                use ::diesel::prelude::*;
                ::core::result::Result::Ok(
                    Self::query()
                        .filter(#table::#id_field.eq(id))
                        .select(<Self as ::diesel::SelectableHelper<#backend>>::as_select())
                        .first::<Self>(conn)
                        .optional()?,
                )
            }

            /// Find every row whose id is in `ids`.
            ///
            /// # Arguments
            ///
            /// * `conn` - The connection to run on. Generic in the
            ///   connection and concrete in the backend, so one binary can hold two
            ///   pools.
            /// * `ids` - The keys to look for. One statement rather than one per
            ///   id, which is the whole reason this exists.
            ///
            /// # Errors
            /// [`::toolbox_db::DbError::Query`] when the statement fails.
            pub fn find_by_ids<__C>(
                conn: &mut __C,
                ids: &[#id_type],
            ) -> ::toolbox_db::DbResult<::std::vec::Vec<Self>>
            where
                __C: ::diesel::connection::LoadConnection<Backend = #backend>,
            {
                use ::diesel::prelude::*;
                ::core::result::Result::Ok(
                    Self::query()
                        .filter(#table::#id_field.eq_any(ids.to_vec()))
                        .select(<Self as ::diesel::SelectableHelper<#backend>>::as_select())
                        .load::<Self>(conn)?,
                )
            }

            /// Whether a row with this id exists.
            ///
            /// # Arguments
            ///
            /// * `conn` - The connection to run on. Generic in the
            ///   connection and concrete in the backend, so one binary can hold two
            ///   pools.
            /// * `id` - The primary key to test for. Cheaper than `find_by_id`
            ///   when the row itself is not needed.
            ///
            /// # Errors
            /// [`::toolbox_db::DbError::Query`] when the statement fails.
            pub fn exists<__C>(conn: &mut __C, id: &#id_type) -> ::toolbox_db::DbResult<bool>
            where
                __C: ::diesel::connection::LoadConnection<Backend = #backend>,
            {
                use ::diesel::prelude::*;
                let __n: i64 = Self::query()
                    .filter(#table::#id_field.eq(id))
                    .count()
                    .get_result(conn)?;
                ::core::result::Result::Ok(__n > 0)
            }

            /// How many rows there are.
            ///
            /// # Arguments
            ///
            /// * `conn` - The connection to run on. Generic in the
            ///   connection and concrete in the backend, so one binary can hold two
            ///   pools.
            ///
            /// # Errors
            /// [`::toolbox_db::DbError::Query`] when the statement fails.
            pub fn count<__C>(conn: &mut __C) -> ::toolbox_db::DbResult<i64>
            where
                __C: ::diesel::connection::LoadConnection<Backend = #backend>,
            {
                use ::diesel::prelude::*;
                ::core::result::Result::Ok(Self::query().count().get_result(conn)?)
            }

            /// One page of rows, ordered by the request's sort.
            ///
            /// # Arguments
            ///
            /// * `conn` - The connection to run on. Generic in the
            ///   connection and concrete in the backend, so one binary can hold two
            ///   pools.
            /// * `request` - The window and ordering. Sort fields are checked
            ///   against the entity's allowlist, never interpolated into SQL.
            ///
            /// # Errors
            /// [`::toolbox_db::DbError::InvalidSortField`] when the request
            /// sorts by a field this entity does not declare sortable, or
            /// [`::toolbox_db::DbError::Query`] when the statement fails.
            pub fn page<__C>(
                conn: &mut __C,
                request: &::toolbox_core::PageRequest,
            ) -> ::toolbox_db::DbResult<::toolbox_core::Page<Self>>
            where
                __C: ::diesel::connection::LoadConnection<Backend = #backend>,
            {
                use ::diesel::prelude::*;
                use ::toolbox_db::Paginate as _;

                ::toolbox_db::sort::validate(request.sort(), Self::sortable_fields())?;
                let mut __q = Self::query();
                for __item in request.sort().items() {
                    __q = match (__item.field.as_str(), __item.direction) {
                        #(#sort_arms)*
                        _ => __q,
                    };
                }
                let __page: ::toolbox_core::Page<Self> = __q
                    .select(<Self as ::diesel::SelectableHelper<#backend>>::as_select())
                    .paginate(request)
                    .load_page::<Self, __C>(conn)?;
                if __page.is_empty() {
                    // The window count rides on the rows, so an offset past the
                    // end brings back no total. `page()` adds no filter beyond
                    // the soft-delete one, so the table count is the real total.
                    let __total = Self::count(conn)?;
                    if __total != __page.total() {
                        return ::core::result::Result::Ok(::toolbox_core::Page::new(
                            ::std::vec::Vec::new(),
                            ::core::clone::Clone::clone(request),
                            __total,
                        ));
                    }
                }
                ::core::result::Result::Ok(__page)
            }

            /// Insert or update this row, returning it as stored.
            ///
            /// Runs in a transaction, because deciding whether the row exists
            /// and then writing it is a read-modify-write: without one, two
            /// concurrent saves of the same new row both decide to insert.
            /// Nested inside a caller's transaction it becomes a savepoint,
            /// which is what makes `save_all` work.
            ///
            /// # Arguments
            ///
            /// * `conn` - The connection to run on. Generic in the
            ///   connection and concrete in the backend, so one binary can hold two
            ///   pools.
            ///
            /// # Errors
            /// [`::toolbox_db::DbError::Conflict`] when an optimistic-locking
            /// check matched no rows, or
            /// [`::toolbox_db::DbError::Query`] when the statement fails.
            pub fn save<__C>(&self, conn: &mut __C) -> ::toolbox_db::DbResult<Self>
            where
                __C: ::diesel::connection::LoadConnection<Backend = #backend>
                    + ::diesel::Connection<Backend = #backend>,
            {
                use ::diesel::prelude::*;
                let mut __row = ::core::clone::Clone::clone(self);
                conn.transaction(|conn| {
                    #save_body
                })
            }

            /// Save every row, in one transaction.
            ///
            /// # Arguments
            ///
            /// * `conn` - The connection to run on. Generic in the
            ///   connection and concrete in the backend, so one binary can hold two
            ///   pools.
            /// * `items` - The rows to save. One transaction for the batch, so
            ///   the first failure rolls back everything before it.
            ///
            /// # Errors
            /// As [`Self::save`]; the first failure rolls the whole batch back.
            pub fn save_all<__C>(
                conn: &mut __C,
                items: &[Self],
            ) -> ::toolbox_db::DbResult<::std::vec::Vec<Self>>
            where
                __C: ::diesel::connection::LoadConnection<Backend = #backend>
                    + ::diesel::Connection<Backend = #backend>,
            {
                conn.transaction(|__c| {
                    let mut __out = ::std::vec::Vec::with_capacity(items.len());
                    for __item in items {
                        __out.push(__item.save(__c)?);
                    }
                    ::core::result::Result::Ok(__out)
                })
            }

            /// Delete one row by id, returning how many rows changed.
            ///
            /// # Arguments
            ///
            /// * `conn` - The connection to run on. Generic in the
            ///   connection and concrete in the backend, so one binary can hold two
            ///   pools.
            /// * `id` - The primary key to delete. An `UPDATE` when the entity
            ///   soft-deletes, a `DELETE` otherwise.
            ///
            /// # Errors
            /// [`::toolbox_db::DbError::Query`] when the statement fails.
            pub fn delete_by_id<__C>(
                conn: &mut __C,
                id: &#id_type,
            ) -> ::toolbox_db::DbResult<usize>
            where
                __C: ::diesel::connection::LoadConnection<Backend = #backend>,
            {
                use ::diesel::prelude::*;
                #delete_body
            }

            /// Delete every row whose id is in `ids`.
            ///
            /// # Arguments
            ///
            /// * `conn` - The connection to run on. Generic in the
            ///   connection and concrete in the backend, so one binary can hold two
            ///   pools.
            /// * `ids` - The keys to delete, in one statement rather than one
            ///   round trip per row.
            ///
            /// # Errors
            /// [`::toolbox_db::DbError::Query`] when the statement fails.
            pub fn delete_by_ids<__C>(
                conn: &mut __C,
                ids: &[#id_type],
            ) -> ::toolbox_db::DbResult<usize>
            where
                __C: ::diesel::connection::LoadConnection<Backend = #backend>,
            {
                use ::diesel::prelude::*;
                #delete_many_body
            }

            /// Delete every row. Unconditional, including soft-deleted ones.
            ///
            /// # Arguments
            ///
            /// * `conn` - The connection to run on. Generic in the
            ///   connection and concrete in the backend, so one binary can hold two
            ///   pools.
            ///
            /// # Errors
            /// [`::toolbox_db::DbError::Query`] when the statement fails.
            pub fn truncate<__C>(conn: &mut __C) -> ::toolbox_db::DbResult<usize>
            where
                __C: ::diesel::connection::LoadConnection<Backend = #backend>,
            {
                use ::diesel::prelude::*;
                ::core::result::Result::Ok(::diesel::delete(#table::table).execute(conn)?)
            }
        }

        #[automatically_derived]
        impl ::toolbox_db::Entity for #ident {
            type Id = #id_type;
            type Table = #table::table;

            fn id(&self) -> ::core::option::Option<&Self::Id> {
                #id_accessor
            }
        }
    }
}

/// The soft-delete filter, folded into every read.
///
/// # Arguments
///
/// * `cfg` - Everything the `#[entity(..)]` attribute declared, already
///   resolved against the struct.
fn alive_filter(cfg: &EntityConfig) -> TokenStream {
    let table = &cfg.table;
    cfg.soft_delete
        .as_ref()
        .map_or_else(TokenStream::new, |col| {
            quote! { let __q = __q.filter(#table::#col.is_null()); }
        })
}

/// One match arm per `sortable(..)` column, which is what keeps a sort
/// field name out of the generated SQL.
///
/// # Arguments
///
/// * `cfg` - Everything the `#[entity(..)]` attribute declared, already
///   resolved against the struct.
fn sort_arms(cfg: &EntityConfig) -> Vec<TokenStream> {
    let table = &cfg.table;
    cfg.sortable
        .iter()
        .map(|col| {
            let name = col.to_string();
            quote! {
                (#name, ::toolbox_core::SortDirection::Asc) =>
                    __q.then_order_by(#table::#col.asc()),
                (#name, ::toolbox_core::SortDirection::Desc) =>
                    __q.then_order_by(#table::#col.desc()),
            }
        })
        .collect()
}

/// Set `created_at`. Emitted only into the branch that is doing an insert -
/// `Entity::id().is_none()` cannot answer that question for an entity whose id
/// the caller supplies.
///
/// # Arguments
///
/// * `cfg` - Everything the `#[entity(..)]` attribute declared, already
///   resolved against the struct.
fn touch_created(cfg: &EntityConfig) -> TokenStream {
    cfg.timestamps.as_ref().map_or_else(TokenStream::new, |ts| {
        let created = &ts.created_at;
        quote! { __row.#created = ::toolbox_db::Now::now(); }
    })
}

/// Set `updated_at`, on every save.
///
/// # Arguments
///
/// * `cfg` - Everything the `#[entity(..)]` attribute declared, already
///   resolved against the struct.
fn touch_updated(cfg: &EntityConfig) -> TokenStream {
    cfg.timestamps.as_ref().map_or_else(TokenStream::new, |ts| {
        let updated = &ts.updated_at;
        quote! { __row.#updated = ::toolbox_db::Now::now(); }
    })
}

/// The insert-or-update body, which is the one place the backends differ.
///
/// # Arguments
///
/// * `cfg` - Everything the `#[entity(..)]` attribute declared, already
///   resolved against the struct.
fn save_body(cfg: &EntityConfig) -> TokenStream {
    let table = &cfg.table;
    let id_field = &cfg.id_field;
    let backend = &cfg.backend;
    let touch_created = touch_created(cfg);
    let touch_updated = touch_updated(cfg);

    // `AsChangeset` skips `Option<T>` fields that are `None` rather than
    // nulling them, which is diesel's behaviour and a common surprise; a
    // nullable column that must be clearable needs `treat_none_as_null` on the
    // consumer's own model.
    let update = cfg.version.as_ref().map_or_else(
        || {
            quote! {
                let __changed = ::diesel::update(
                    #table::table.filter(#table::#id_field.eq(&__row.#id_field)),
                )
                .set(&__row)
                .execute(conn)?;
                if __changed == 0 {
                    // The row was there for the existence probe and gone by the
                    // time we wrote it.
                    return ::core::result::Result::Err(::toolbox_db::DbError::NotFound);
                }
            }
        },
        |v| {
            quote! {
                let __expected = __row.#v;
                let ::core::option::Option::Some(__next) = __expected.checked_add(1) else {
                    // The version column has run out of room; optimistic locking
                    // cannot continue on a value it would have to reuse.
                    return ::core::result::Result::Err(
                        ::toolbox_db::DbError::VersionOverflow,
                    );
                };
                __row.#v = __next;
                let __changed = ::diesel::update(
                    #table::table
                        .filter(#table::#id_field.eq(&__row.#id_field))
                        .filter(#table::#v.eq(__expected)),
                )
                .set(&__row)
                .execute(conn)?;
                if __changed == 0 {
                    // Zero rows matched: either the row vanished or someone else
                    // wrote first. Both are a lost update from this caller's view.
                    return ::core::result::Result::Err(::toolbox_db::DbError::Conflict);
                }
            }
        },
    );

    // An update has the whole row in hand, incremented version and all, so it
    // is the stored state; only an insert has to read back what the database
    // filled in.
    let updated_row = quote! { ::core::result::Result::Ok(__row) };
    let reload = quote! {
        Self::find_by_id(conn, &__row.#id_field)?
            .ok_or(::toolbox_db::DbError::NotFound)
    };

    if cfg.autoincrement {
        // The id is assigned by the database, so an insert has to read it back.
        let fetch_inserted = match cfg.dialect {
            Dialect::Returning => quote! {
                #touch_created
                #touch_updated
                ::core::result::Result::Ok(
                    ::diesel::insert_into(#table::table)
                        .values(&__row)
                        .returning(<Self as ::diesel::SelectableHelper<#backend>>::as_returning())
                        .get_result::<Self>(conn)?,
                )
            },
            Dialect::Mysql => quote! {
                #touch_created
                #touch_updated
                // Two statements in one transaction: LAST_INSERT_ID() is scoped
                // to this connection, so `WHERE id = LAST_INSERT_ID()` names the
                // row this INSERT just created even while other sessions insert.
                ::diesel::insert_into(#table::table).values(&__row).execute(conn)?;
                ::core::result::Result::Ok(
                    #table::table
                        .filter(#table::#id_field.eq(
                            ::diesel::dsl::sql::<
                                ::diesel::dsl::SqlTypeOf<#table::#id_field>,
                            >("LAST_INSERT_ID()"),
                        ))
                        .select(<Self as ::diesel::SelectableHelper<#backend>>::as_select())
                        .first::<Self>(conn)?,
                )
            },
        };
        quote! {
            if <Self as ::toolbox_db::Entity>::id(&__row).is_none() {
                #fetch_inserted
            } else {
                #touch_updated
                #update
                #updated_row
            }
        }
    } else {
        quote! {
            // Probe the table directly rather than through `query()`: a
            // soft-deleted row still occupies its primary key, so inserting
            // over it would be a duplicate-key error.
            let __existing: i64 = #table::table
                .filter(#table::#id_field.eq(&__row.#id_field))
                .count()
                .get_result(conn)?;
            if __existing > 0 {
                #touch_updated
                #update
                #updated_row
            } else {
                #touch_created
                #touch_updated
                ::diesel::insert_into(#table::table).values(&__row).execute(conn)?;
                #reload
            }
        }
    }
}

/// Delete one row: an `UPDATE` when the entity soft-deletes, a `DELETE`
/// otherwise.
///
/// # Arguments
///
/// * `cfg` - Everything the `#[entity(..)]` attribute declared, already
///   resolved against the struct.
fn delete_body(cfg: &EntityConfig) -> TokenStream {
    let table = &cfg.table;
    let id_field = &cfg.id_field;
    cfg.soft_delete.as_ref().map_or_else(
        || {
            quote! {
                ::core::result::Result::Ok(
                    ::diesel::delete(#table::table.filter(#table::#id_field.eq(id)))
                        .execute(conn)?,
                )
            }
        },
        |col| {
            let ty = cfg
                .soft_delete_type
                .as_ref()
                .expect("parsed alongside the column");
            quote! {
                let __now: #ty = ::toolbox_db::Now::now();
                ::core::result::Result::Ok(
                    ::diesel::update(
                        #table::table
                            .filter(#table::#id_field.eq(id))
                            .filter(#table::#col.is_null()),
                    )
                    .set(#table::#col.eq(::core::option::Option::Some(__now)))
                    .execute(conn)?,
                )
            }
        },
    )
}

/// The same, for a slice of ids in one statement.
///
/// # Arguments
///
/// * `cfg` - Everything the `#[entity(..)]` attribute declared, already
///   resolved against the struct.
fn delete_many_body(cfg: &EntityConfig) -> TokenStream {
    let table = &cfg.table;
    let id_field = &cfg.id_field;
    cfg.soft_delete.as_ref().map_or_else(
        || {
            quote! {
                ::core::result::Result::Ok(
                    ::diesel::delete(#table::table.filter(#table::#id_field.eq_any(ids.to_vec())))
                        .execute(conn)?,
                )
            }
        },
        |col| {
            let ty = cfg
                .soft_delete_type
                .as_ref()
                .expect("parsed alongside the column");
            quote! {
                let __now: #ty = ::toolbox_db::Now::now();
                ::core::result::Result::Ok(
                    ::diesel::update(
                        #table::table
                            .filter(#table::#id_field.eq_any(ids.to_vec()))
                            .filter(#table::#col.is_null()),
                    )
                    .set(#table::#col.eq(::core::option::Option::Some(__now)))
                    .execute(conn)?,
                )
            }
        },
    )
}

/// `Entity::id` returns `None` when the row has not been inserted yet, which
/// for an autoincrement key means the sentinel zero.
///
/// # Arguments
///
/// * `cfg` - Everything the `#[entity(..)]` attribute declared, already
///   resolved against the struct.
fn id_accessor(cfg: &EntityConfig) -> TokenStream {
    let id_field = &cfg.id_field;
    if cfg.autoincrement {
        let id_type = &cfg.id_type;
        // Fully qualified: a bare `Default::default()` is ambiguous in any
        // crate where another `PartialEq` impl for the id type is in scope -
        // `serde_json::Value` is the one that finds this.
        quote! {
            if self.#id_field == <#id_type as ::core::default::Default>::default() {
                ::core::option::Option::None
            } else {
                ::core::option::Option::Some(&self.#id_field)
            }
        }
    } else {
        quote! { ::core::option::Option::Some(&self.#id_field) }
    }
}
