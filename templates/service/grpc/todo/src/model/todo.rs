//! A todo.

use diesel::prelude::*;

use crate::{Backend, Timestamp, proto, schema::todos};

/// A todo.
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
