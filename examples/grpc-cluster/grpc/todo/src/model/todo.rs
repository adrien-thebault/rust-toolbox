//! A todo.

use diesel::prelude::*;
use toolbox_db::DbError;

use crate::{Backend, Timestamp, proto, schema::todos};

/// A todo.
///
/// Every option on the derive is exercised here on purpose: this is what
/// stands between the macro and a silent regression.
#[derive(
    Debug, Clone, PartialEq, Eq, Queryable, Selectable, Insertable, AsChangeset, toolbox_db::Entity,
)]
#[diesel(table_name = todos)]
#[diesel(check_for_backend(Backend))]
#[entity(
    backend = crate::Backend,
    id = id,
    autoincrement,
    timestamps,
    soft_delete = deleted_at,
    version = version,
    sortable(id, title, created_at),
)]
pub struct Todo {
    /// Assigned by the database, so it is left out of inserts.
    #[diesel(skip_insertion)]
    pub id: i32,
    /// What to do.
    pub title: String,
    /// Whether it is done.
    pub done: bool,
    /// When it was created.
    pub created_at: Timestamp,
    /// When it was last changed.
    pub updated_at: Timestamp,
    /// When it was deleted, if it was.
    pub deleted_at: Option<Timestamp>,
    /// Bumped on every save, for optimistic locking.
    pub version: i32,
}

impl Todo {
    /// A new todo, before the database assigns it an id.
    ///
    /// # Arguments
    ///
    /// * `title` - What to do. The timestamps are placeholders; `timestamps` on
    ///   the derive overwrites them on save.
    #[must_use]
    pub fn new(title: impl Into<String>) -> Self {
        let epoch = chrono::DateTime::from_timestamp(0, 0)
            .unwrap_or_default()
            .naive_utc();
        Self {
            id: 0,
            title: title.into(),
            done: false,
            created_at: epoch,
            updated_at: epoch,
            deleted_at: None,
            version: 0,
        }
    }

    /// Todos whose title contains `needle`, paged.
    ///
    /// A hand-written filter composing with the toolbox's pagination, which is
    /// the case a naive repository could not do: the moment you needed
    /// a `WHERE` clause you lost paging, sorting and error mapping.
    ///
    /// # Arguments
    ///
    /// * `conn` - The connection to load on.
    /// * `needle` - Matched with `LIKE %needle%`.
    /// * `request` - The window and sort to apply.
    ///
    /// # Errors
    /// [`DbError`] when the query fails or the sort names an undeclared field.
    pub fn search<C>(
        conn: &mut C,
        needle: &str,
        request: &toolbox_core::PageRequest,
    ) -> Result<toolbox_core::Page<Self>, DbError>
    where
        C: diesel::connection::LoadConnection<Backend = Backend>,
    {
        use toolbox_db::Paginate as _;

        toolbox_db::pagination::validate(request.sort(), Self::sortable_fields())?;
        Self::query()
            .filter(todos::title.like(format!("%{needle}%")))
            .select(Self::as_select())
            .paginate(request)
            .load_page::<Self, C>(conn)
    }
}

/// How the entity is put on the wire. Here rather than beside a service,
/// because every service in the domain sends the same shape and the entity is
/// what they all have in common.
impl From<Todo> for proto::Todo {
    fn from(t: Todo) -> Self {
        Self {
            id: t.id,
            title: t.title,
            done: t.done,
            version: t.version,
        }
    }
}
