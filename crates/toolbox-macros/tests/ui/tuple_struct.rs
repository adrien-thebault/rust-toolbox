use toolbox_db::Entity;

pub type Backend = diesel::sqlite::Sqlite;

#[derive(Clone, Entity)]
#[entity(backend = Backend, id = id)]
pub struct Item(i32, String);

fn main() {}
