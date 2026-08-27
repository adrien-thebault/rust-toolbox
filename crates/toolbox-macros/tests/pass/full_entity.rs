use diesel::prelude::*;
use toolbox_db::Entity;

pub type Backend = diesel::sqlite::Sqlite;

diesel::table! {
    items (id) {
        id -> Integer,
        title -> Text,
        created_at -> Timestamp,
        updated_at -> Timestamp,
        deleted_at -> Nullable<Timestamp>,
        version -> Integer,
    }
}

#[derive(Clone, Queryable, Selectable, Insertable, AsChangeset, Entity)]
#[diesel(table_name = items)]
#[entity(
    backend = Backend,
    id = id,
    timestamps,
    soft_delete = deleted_at,
    version = version,
    sortable(id, title, created_at),
)]
pub struct Item {
    pub id: i32,
    pub title: String,
    pub created_at: chrono::NaiveDateTime,
    pub updated_at: chrono::NaiveDateTime,
    pub deleted_at: Option<chrono::NaiveDateTime>,
    pub version: i32,
}

fn main() {
    assert_eq!(Item::sortable_fields(), ["id", "title", "created_at"]);
}
