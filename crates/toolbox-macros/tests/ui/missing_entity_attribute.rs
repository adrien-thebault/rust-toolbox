use diesel::prelude::*;
use toolbox_db::Entity;

diesel::table! {
    items (id) {
        id -> Integer,
        title -> Text,
    }
}

#[derive(Clone, Queryable, Selectable, Insertable, AsChangeset, Entity)]
#[diesel(table_name = items)]
pub struct Item {
    pub id: i32,
    pub title: String,
}

fn main() {}
