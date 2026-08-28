//! The todo domain.
//!
//! One crate per **domain**, not per gRPC service: `service/` holds as many
//! proto services as this domain exposes and `model/` as many entities, and
//! they share one schema, one migration set and one pool. A crate per service
//! would multiply build graphs and deployment units for isolation the module
//! boundary already gave you.
//!
//! Nothing in here knows that an HTTP gateway exists, which is what lets the
//! two be deployed and scaled independently.

pub mod model;
pub mod schema;
pub mod service;

pub use model::Todo;
pub use service::{TodoService, TodoServiceError};

/// The database backend, named **once** for the whole crate.
pub type Backend = diesel::sqlite::Sqlite;

/// The connection type, following from [`Backend`].
pub type Connection = diesel::sqlite::SqliteConnection;

/// The timestamp type, named **once** for the whole crate.
pub type Timestamp = chrono::NaiveDateTime;

/// This domain's migrations, applied by the caller at startup.
pub const MIGRATIONS: toolbox_db::EmbeddedMigrations = toolbox_db::embed_migrations!("migrations");

/// The generated protobuf types and service stubs.
pub mod proto {
    #![allow(missing_docs, clippy::pedantic, clippy::all)]
    tonic::include_proto!("todo.v1");

    /// The descriptor set, so `grpcurl` works without the protos to hand.
    pub const DESCRIPTOR: &[u8] = tonic::include_file_descriptor_set!("todo_descriptor");
}
