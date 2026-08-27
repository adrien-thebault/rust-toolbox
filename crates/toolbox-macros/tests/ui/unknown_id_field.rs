use diesel::prelude::*;
use toolbox_db::Entity;

pub type Backend = diesel::sqlite::Sqlite;

diesel::table! {
    items (id) {
        id -> Integer,
        title -> Text,
        created_at -> Timestamp,
        deleted_at -> Nullable<Timestamp>,
        version -> Integer,
    }
}

#[derive(Clone, Queryable, Selectable, Insertable, AsChangeset, Entity)]
#[diesel(table_name = items)]
#[entity(backend = Backend, id = nope)]
pub struct Item {
    pub id: i32,
    pub title: String,
    pub created_at: chrono::NaiveDateTime,
    pub deleted_at: Option<chrono::NaiveDateTime>,
    pub version: i32,

}

fn main() {}
