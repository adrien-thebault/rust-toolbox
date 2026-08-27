use toolbox_db::Entity;

pub type Backend = diesel::sqlite::Sqlite;

#[derive(Clone, Entity)]
#[entity(backend = Backend, id = id)]
pub struct Item {
    pub id: i32,
}

fn main() {}
