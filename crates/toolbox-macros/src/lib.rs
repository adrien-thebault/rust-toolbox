//! Derive macros for the toolbox.
//!
//! `#[derive(Entity)]` replaces four `macro_rules!` macros and seven traits
//! with one derive, and - unlike `macro_rules!` - it can point a compile error
//! at the offending token. The error messages are the product, so every misuse
//! has a committed `trybuild` case.
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
/// - `dialect = returning | mysql` - which SQL to use where the backends
///   genuinely differ, which is only reading back an autoincrement key.
///   Defaults to `returning` (PostgreSQL, SQLite 3.35+). A proc macro sees the
///   token `crate::Backend`, not the type it resolves to, so this cannot be
///   inferred from `backend`.
/// - `autoincrement` - the database assigns the id.
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
