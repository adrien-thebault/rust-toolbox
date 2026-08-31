//! Derive macros for the toolbox.
//!
//! `#[derive(Entity)]` generates a diesel entity's inherent CRUD methods from
//! one attribute and, unlike a `macro_rules!`, points a compile error at the
//! offending token. The error messages are the product, so every misuse has a
//! committed `trybuild` case.
//!
//! This file is the registry: one `#[proc_macro*]` entry point per macro,
//! each delegating to a module of the same name. A second macro is a new
//! module next to `entity`, not another pair of `parse`/`expand` files
//! fighting for the same names.

mod entity;

use proc_macro::TokenStream;
use syn::{DeriveInput, parse_macro_input};

/// Generate inherent CRUD methods on a diesel entity.
///
/// ```ignore
/// #[derive(Clone, Queryable, Selectable, Insertable, AsChangeset, Entity)]
/// #[diesel(table_name = crate::schema::events)]
/// #[entity(
///     backend     = crate::Backend,
///     id          = id,
///     timestamps,
///     soft_delete = deleted_at,
///     version     = version,
///     sortable(id, title, created_at),
/// )]
/// pub struct Event { /* .. */ }
/// ```
///
/// # Arguments
///
/// * `input` - The struct the attribute is attached to, as tokens.
///
/// # Options
///
/// - `backend = <path>` (required) - the diesel backend **type**, not a
///   feature. An alias like `crate::Backend` is the point: it is the one place
///   the backend is named, so swapping it is a one-line change.
/// - `id = <field>` (required) - the primary key field.
/// - `autoincrement` - the database assigns the id, so an insert reads it back:
///   `INSERT .. RETURNING` by default (PostgreSQL, SQLite 3.35+), or
///   `autoincrement = last_insert_id` for MySQL's
///   `SELECT .. WHERE id = LAST_INSERT_ID()`. A proc macro sees the token
///   `crate::Backend`, not the type it resolves to, so which one cannot be
///   inferred from `backend`.
/// - `timestamps` - maintain `created_at` and `updated_at` through
///   [`toolbox_db::Now`](../toolbox_db/trait.Now.html).
/// - `soft_delete = <field>` - a nullable column; deletes become updates and
///   every read filters on it.
/// - `version = <field>` - optimistic locking; a save whose version check
///   matches no rows is `DbError::Conflict`.
/// - `sortable(a, b, ..)` - the allowlist `page()` validates against. Anything
///   else is `DbError::InvalidSortField`, never interpolated SQL.
#[proc_macro_derive(Entity, attributes(entity))]
pub fn derive_entity(input: TokenStream) -> TokenStream {
    entity::derive(&parse_macro_input!(input as DeriveInput)).into()
}
