use diesel::prelude::*;
use toolbox_db::Entity;

pub type Backend = diesel::sqlite::Sqlite;

diesel::table! {
    items (id) {
        id -> Integer,
        title -> Text,
    }
}

#[derive(Clone, Queryable, Selectable, Insertable, AsChangeset, Entity)]
#[diesel(table_name = items)]
#[entity(backend = Backend, id = id, timestamps)]
pub struct Item {
    pub id: i32,
    pub title: String,
}

fn main() {}
