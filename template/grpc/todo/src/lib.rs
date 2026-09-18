//! The todo domain.
//!
//! One crate per **domain**, not per gRPC service: `service/` holds as many
//! proto services as this domain exposes and `model/` as many entities, and
//! they share one schema, one migration set and one pool. A second domain is a
//! sibling directory under `grpc/`, which the workspace picks up with no edit.

pub mod auth;
pub mod model;
pub mod schema;
pub mod service;

pub use model::Todo;
pub use service::{TodoService, TodoServiceError};

// `Backend` and `Connection`, named **once** for the whole crate. Swapping
// backends is this line plus the connection URL, because every entity says
// `backend = crate::Backend` rather than naming a diesel type. Timestamp
// columns are plain `chrono::NaiveDateTime`.
{% if database == "postgres" %}toolbox::db::postgres_backend!();
{% elsif database == "mariadb" %}toolbox::db::mysql_backend!();
{% else %}toolbox::db::sqlite_backend!();
{% endif %}
/// The event-bus topic every todo mutation is published on, and `WatchTodos`
/// subscribes to.
pub const TODOS_TOPIC: &str = "todos";

/// The CloudEvents `source` for every todo event this domain emits.
pub const EVENT_SOURCE: &str = "/todo-service";

/// This domain's migrations, applied by the binary at startup.
pub const MIGRATIONS: toolbox::db::EmbeddedMigrations =
    toolbox::db::embed_migrations!("migrations");

/// The generated protobuf types and service stubs.
pub mod proto {
    #![allow(
        missing_docs,
        clippy::all,
        clippy::pedantic,
        clippy::missing_docs_in_private_items
    )]
    tonic::include_proto!("todo.v1");

    /// The descriptor set, so `grpcurl` works without the protos to hand.
    pub const DESCRIPTOR: &[u8] = tonic::include_file_descriptor_set!("todo_descriptor");
}
