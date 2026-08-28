//! The todo domain.
//!
//! One crate per **domain**, not per gRPC service: `service/` holds as many
//! proto services as this domain exposes and `model/` as many entities, and
//! they share one schema, one migration set and one pool. A second domain is a
//! sibling directory under `grpc/`, which the workspace picks up with no edit.

pub mod model;
pub mod schema;
pub mod service;

pub use model::Todo;
pub use service::{TodoService, TodoServiceError};

/// The database backend, named **once** for the whole crate.
///
/// Swapping backends is this line plus the connection URL, because every entity
/// says `backend = crate::Backend` rather than naming a diesel type.
{% if database == "postgres" %}pub type Backend = diesel::pg::Pg;

/// The connection type, following from [`Backend`].
pub type Connection = diesel::pg::PgConnection;
{% else %}pub type Backend = diesel::sqlite::Sqlite;

/// The connection type, following from [`Backend`].
pub type Connection = diesel::sqlite::SqliteConnection;
{% endif %}
/// The timestamp type, named **once** for the whole crate.
///
/// The eventual chrono-to-jiff move is this line plus an impl of
/// `toolbox_db::Now`, rather than every entity in the tree.
pub type Timestamp = chrono::NaiveDateTime;

/// This domain's migrations, applied by the binary at startup.
pub const MIGRATIONS: toolbox_db::EmbeddedMigrations = toolbox_db::embed_migrations!("migrations");

/// The generated protobuf types and service stubs.
pub mod proto {
    #![allow(missing_docs, clippy::pedantic, clippy::all)]
    tonic::include_proto!("todo.v1");

    /// The descriptor set, so `grpcurl` works without the protos to hand.
    pub const DESCRIPTOR: &[u8] = tonic::include_file_descriptor_set!("todo_descriptor");
}
