//! Parsing the `#[entity(..)]` attribute.
//!
//! Written against `syn` directly rather than through `darling` because the
//! error spans *are* the product here: every misuse has a committed
//! `trybuild` case asserting exactly where the caret lands.

use proc_macro2::Span;
use syn::{
    Attribute, Data, DeriveInput, Expr, Fields, Ident, Path, Type, punctuated::Punctuated,
    spanned::Spanned,
};

/// Which SQL to use where the backends genuinely differ.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Dialect {
    /// `INSERT .. RETURNING`, one statement. PostgreSQL and SQLite 3.35+.
    Returning,
    /// `INSERT` then `SELECT .. WHERE id = LAST_INSERT_ID()`, in a
    /// transaction because the two statements must see the same connection
    /// state.
    Mysql,
}

/// Everything `#[entity(..)]` declared, resolved against the struct.
pub struct EntityConfig {
    /// The struct the derive is on.
    pub ident: Ident,
    /// Its `diesel::table!` path.
    pub table: Path,
    /// The backend type the queries are generic over.
    pub backend: Path,
    /// Which SQL dialect to emit for inserts.
    pub dialect: Dialect,
    /// The primary-key column.
    pub id_field: Ident,
    /// The primary-key column's type.
    pub id_type: Type,
    /// Whether the database assigns the id.
    pub autoincrement: bool,
    /// The created/updated columns, if `timestamps` was set.
    pub timestamps: Option<Timestamps>,
    /// The nullable timestamp column that marks a row deleted, if any.
    pub soft_delete: Option<Ident>,
    /// `T` from the soft-delete column's `Option<T>`.
    pub soft_delete_type: Option<Type>,
    /// The optimistic-locking version column, if any.
    pub version: Option<Ident>,
    /// Columns the generated `page` may sort on.
    pub sortable: Vec<Ident>,
}

/// The two columns `timestamps` maintains.
pub struct Timestamps {
    /// The insert-time column.
    pub created_at: Ident,
    /// The column touched on every write.
    pub updated_at: Ident,
}

/// One `key = value` or bare `key` or `key(a, b)` item inside `#[entity(..)]`.
enum Item {
    /// `backend = <path>`.
    Backend(Path),
    /// `dialect = <ident>`.
    Dialect(Ident),
    /// `id = <ident>`.
    Id(Ident),
    /// bare `autoincrement`.
    Autoincrement,
    /// bare `timestamps`.
    Timestamps,
    /// `soft_delete = <ident>`.
    SoftDelete(Ident),
    /// `version = <ident>`.
    Version(Ident),
    /// `sortable(a, b, ...)`.
    Sortable(Vec<Ident>),
}

/// Resolve `#[entity(..)]` against the struct it is attached to.
///
/// Every error here is a `trybuild` case, so the span matters as much as the
/// message.
///
/// # Arguments
///
/// * `input` - The struct carrying the attribute. Its fields are what every
///   option is resolved against.
pub fn parse(input: &DeriveInput) -> syn::Result<EntityConfig> {
    let fields = struct_fields(input)?;
    let table = table_path(&input.attrs, input.ident.span())?;

    let entity_attr = input
        .attrs
        .iter()
        .find(|a| a.path().is_ident("entity"))
        .ok_or_else(|| {
            syn::Error::new(
                input.ident.span(),
                "missing #[entity(..)]; #[derive(Entity)] needs at least \
                 `backend = <path>` and `id = <field>`",
            )
        })?;

    let mut backend = None;
    let mut dialect_ident: Option<Ident> = None;
    let mut id = None;
    let mut autoincrement = false;
    let mut timestamps = false;
    let mut soft_delete = None;
    let mut version = None;
    let mut sortable: Option<Vec<Ident>> = None;

    for item in parse_items(entity_attr)? {
        match item {
            Item::Backend(p) => backend = Some(p),
            Item::Dialect(i) => dialect_ident = Some(i),
            Item::Id(i) => id = Some(i),
            Item::Autoincrement => autoincrement = true,
            Item::Timestamps => timestamps = true,
            Item::SoftDelete(i) => soft_delete = Some(i),
            Item::Version(i) => version = Some(i),
            Item::Sortable(v) => sortable = Some(v),
        }
    }

    let attr_span = entity_attr.span();
    let backend = backend.ok_or_else(|| {
        syn::Error::new(
            attr_span,
            "`#[entity(..)]` needs `backend = <path>`, e.g. `backend = crate::Backend`",
        )
    })?;
    let id_field = id.ok_or_else(|| {
        syn::Error::new(
            attr_span,
            "`#[entity(..)]` needs `id = <field>` naming the primary key",
        )
    })?;

    let dialect = match dialect_ident {
        None => Dialect::Returning,
        Some(i) if i == "returning" => Dialect::Returning,
        Some(i) if i == "mysql" => Dialect::Mysql,
        Some(i) => {
            return Err(syn::Error::new(
                i.span(),
                "unknown dialect; expected `returning` (PostgreSQL, SQLite 3.35+) or `mysql`",
            ));
        }
    };

    let id_type = field_type(&fields, &id_field, "id")?;

    // An autoincrement key is assigned by the database, so it must not be sent
    // in the INSERT. Diesel's own attribute is how that is expressed, and
    // silently inserting a zero instead is the failure this catches.
    if autoincrement
        && !fields
            .iter()
            .any(|f| f.ident == id_field && f.skip_insertion)
    {
        return Err(syn::Error::new(
            id_field.span(),
            format!(
                "`autoincrement` needs the database to assign `{id_field}`, so the column must \
                 be left out of inserts; add #[diesel(skip_insertion)] to the `{id_field}` field"
            ),
        ));
    }

    let timestamps = if timestamps {
        let created_at = require_field(&fields, "created_at", attr_span, "timestamps")?;
        let updated_at = require_field(&fields, "updated_at", attr_span, "timestamps")?;
        Some(Timestamps {
            created_at,
            updated_at,
        })
    } else {
        None
    };

    let soft_delete_type = match &soft_delete {
        None => None,
        Some(f) => {
            let ty = field_type(&fields, f, "soft_delete")?;
            Some(option_inner(&ty).ok_or_else(|| {
                syn::Error::new(
                    f.span(),
                    format!(
                        "`soft_delete` names `{f}`, whose type is not `Option<_>`; a column \
                         that marks a row deleted has to be nullable"
                    ),
                )
            })?)
        }
    };
    if let Some(f) = &version {
        let ty = field_type(&fields, f, "version")?;
        if !is_integer(&ty) {
            return Err(syn::Error::new(
                f.span(),
                format!(
                    "`version` names `{f}`, whose type is not an integer; optimistic locking \
                     increments the column, so it has to be one of i16, i32, i64, u16, u32, u64"
                ),
            ));
        }
    }

    let sortable = match sortable {
        None => Vec::new(),
        Some(names) => {
            for n in &names {
                field_type(&fields, n, "sortable")?;
            }
            names
        }
    };

    Ok(EntityConfig {
        ident: input.ident.clone(),
        table,
        backend,
        dialect,
        id_field,
        id_type,
        autoincrement,
        timestamps,
        soft_delete,
        soft_delete_type,
        version,
        sortable,
    })
}

/// Split one `#[entity(..)]` attribute into its comma-separated items.
///
/// # Arguments
///
/// * `attr` - One `#[entity(..)]` attribute. Its span is what every error below
///   points at.
fn parse_items(attr: &Attribute) -> syn::Result<Vec<Item>> {
    let mut items = Vec::new();
    attr.parse_nested_meta(|meta| {
        let key = meta
            .path
            .get_ident()
            .cloned()
            .ok_or_else(|| meta.error("expected a bare identifier"))?;

        match key.to_string().as_str() {
            "backend" => items.push(Item::Backend(meta.value()?.parse::<Path>()?)),
            "dialect" => items.push(Item::Dialect(meta.value()?.parse::<Ident>()?)),
            "id" => items.push(Item::Id(meta.value()?.parse::<Ident>()?)),
            "autoincrement" => items.push(Item::Autoincrement),
            "timestamps" => items.push(Item::Timestamps),
            "soft_delete" => items.push(Item::SoftDelete(meta.value()?.parse::<Ident>()?)),
            "version" => items.push(Item::Version(meta.value()?.parse::<Ident>()?)),
            "sortable" => {
                let content;
                syn::parenthesized!(content in meta.input);
                let names = Punctuated::<Ident, syn::Token![,]>::parse_terminated(&content)?;
                items.push(Item::Sortable(names.into_iter().collect()));
            }
            other => {
                return Err(meta.error(format!(
                    "unknown `entity` option `{other}`; expected one of: backend, dialect, id, \
                     autoincrement, timestamps, soft_delete, version, sortable"
                )));
            }
        }
        Ok(())
    })?;
    Ok(items)
}

/// One named field: its name, its type, and whether diesel was told to leave
/// it out of inserts.
pub struct Field {
    /// The field name.
    pub ident: Ident,
    /// The field type.
    pub ty: Type,
    /// Whether `#[diesel(skip_insertion)]` was on it.
    pub skip_insertion: bool,
}

/// The struct's named fields; anything else is rejected here rather than
/// producing an unreadable error deep in the generated code.
///
/// # Arguments
///
/// * `input` - The derive input. A tuple struct or an enum is rejected here.
fn struct_fields(input: &DeriveInput) -> syn::Result<Vec<Field>> {
    let Data::Struct(data) = &input.data else {
        return Err(syn::Error::new(
            input.ident.span(),
            "#[derive(Entity)] only applies to structs with named fields",
        ));
    };
    let Fields::Named(named) = &data.fields else {
        return Err(syn::Error::new(
            input.ident.span(),
            "#[derive(Entity)] needs named fields; a tuple struct has no column names",
        ));
    };
    Ok(named
        .named
        .iter()
        .filter_map(|f| {
            f.ident.clone().map(|ident| Field {
                ident,
                ty: f.ty.clone(),
                skip_insertion: has_skip_insertion(&f.attrs),
            })
        })
        .collect())
}

/// Whether the field carries diesel's `#[diesel(skip_insertion)]`.
///
/// # Arguments
///
/// * `attrs` - The field's own attributes, as diesel wrote them.
fn has_skip_insertion(attrs: &[Attribute]) -> bool {
    let mut found = false;
    for attr in attrs.iter().filter(|a| a.path().is_ident("diesel")) {
        let _ = attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("skip_insertion") {
                found = true;
            } else if meta.input.peek(syn::Token![=]) {
                let _: Expr = meta.value()?.parse()?;
            } else if meta.input.peek(syn::token::Paren) {
                let content;
                syn::parenthesized!(content in meta.input);
                let _: proc_macro2::TokenStream = content.parse()?;
            }
            Ok(())
        });
    }
    found
}

/// Read `table_name` out of diesel's own `#[diesel(..)]` attribute.
///
/// # Arguments
///
/// * `attrs` - The struct's attributes, which is where diesel's `table_name`
///   lives.
/// * `span` - Where to point if there is no `table_name`, since there is no
///   token of our own to blame.
fn table_path(attrs: &[Attribute], span: Span) -> syn::Result<Path> {
    let mut found = None;
    for attr in attrs.iter().filter(|a| a.path().is_ident("diesel")) {
        attr.parse_nested_meta(|meta| {
            if meta.path.is_ident("table_name") {
                found = Some(meta.value()?.parse::<Path>()?);
            } else {
                // Every other diesel option is diesel's business; consume the
                // value if there is one so parsing continues.
                if meta.input.peek(syn::Token![=]) {
                    let _: Expr = meta.value()?.parse()?;
                } else if meta.input.peek(syn::token::Paren) {
                    let content;
                    syn::parenthesized!(content in meta.input);
                    let _: proc_macro2::TokenStream = content.parse()?;
                }
            }
            Ok(())
        })?;
    }
    found.ok_or_else(|| {
        syn::Error::new(
            span,
            "#[derive(Entity)] needs diesel's `#[diesel(table_name = ...)]` to know which \
             table to generate against",
        )
    })
}

/// The declared type of `name`, or an error naming the option that asked
/// for it.
///
/// # Arguments
///
/// * `fields` - Every named field of the struct.
/// * `name` - The field being looked up.
/// * `option` - The `#[entity(..)]` option that asked for it, so the error
///   names the cause rather than just the symptom.
fn field_type(fields: &[Field], name: &Ident, option: &str) -> syn::Result<Type> {
    fields
        .iter()
        .find(|f| &f.ident == name)
        .map(|f| f.ty.clone())
        .ok_or_else(|| {
            syn::Error::new(
                name.span(),
                format!(
                    "`{option}` names `{name}`, which is not a field of this struct; \
                     fields are: {}",
                    field_list(fields)
                ),
            )
        })
}

/// Resolve an option's field name against the struct, listing the real
/// fields when it does not match.
///
/// # Arguments
///
/// * `fields` - Every named field of the struct.
/// * `name` - The field name the option referred to.
/// * `span` - Where to point when it does not exist.
/// * `option` - The option that referred to it, quoted back in the error.
fn require_field(fields: &[Field], name: &str, span: Span, option: &str) -> syn::Result<Ident> {
    fields
        .iter()
        .find(|f| f.ident == name)
        .map(|f| f.ident.clone())
        .ok_or_else(|| {
            syn::Error::new(
                span,
                format!(
                    "`{option}` needs a `{name}` field, which this struct does not have; \
                     fields are: {}",
                    field_list(fields)
                ),
            )
        })
}

/// Whether the type is one of the integer primitives the version column may be.
///
/// # Arguments
///
/// * `ty` - The declared type of the version column.
fn is_integer(ty: &Type) -> bool {
    let Type::Path(p) = ty else { return false };
    p.path.get_ident().is_some_and(|i| {
        matches!(
            i.to_string().as_str(),
            "i16" | "i32" | "i64" | "u16" | "u32" | "u64"
        )
    })
}

/// `T` from `Option<T>`, or `None` when the type is not an `Option`.
///
/// # Arguments
///
/// * `ty` - The declared type, which the soft-delete column requires to be an
///   `Option`.
fn option_inner(ty: &Type) -> Option<Type> {
    let Type::Path(p) = ty else { return None };
    let last = p.path.segments.last()?;
    if last.ident != "Option" {
        return None;
    }
    let syn::PathArguments::AngleBracketed(args) = &last.arguments else {
        return None;
    };
    args.args.iter().find_map(|a| match a {
        syn::GenericArgument::Type(t) => Some(t.clone()),
        _ => None,
    })
}

/// The available field names, for the error message above.
///
/// # Arguments
///
/// * `fields` - Every named field, rendered as a comma-separated list for an
///   error message.
fn field_list(fields: &[Field]) -> String {
    fields
        .iter()
        .map(|f| f.ident.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}
