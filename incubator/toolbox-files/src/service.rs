//! A mountable gRPC file service.
//!
//! The storage logic in the rest of this crate is usable on its own - you can
//! write your own service over [`ingest`](fn@crate::ingest) and
//! [`serve`](crate::serve()) without any of this. This module is the ready-made
//! one, for when you want files to be somebody else's problem.
//!
//! Mount it in one line:
//!
//! ```ignore
//! serve_grpc(cfg, GrpcConfig::default())
//!     .add_service(toolbox_files::service::builder(records, store).build())
//!     .run().await
//! ```
//!
//! # What it owns, and what it does not
//!
//! It owns **file identity**: the key, the size, the hash, the sniffed type,
//! the name it was uploaded under. Your own table owns **file semantics** -
//! who it belongs to, what it is for, who may read it - keyed on the file key.
//!
//! It does not own authorization. [`AuthorizeFile`] defaults to permit-all,
//! because the gateway is the auth layer and this service trusts its caller.
//! Put `toolbox_grpc::require_service_auth` on the channel so that trust is
//! justified.

use std::sync::Arc;

use object_store::ObjectStore;

pub mod grpc;
mod hooks;
pub mod records;
pub mod schema;

pub use grpc::{FileService, FileServiceBuilder};
pub use hooks::{AuthorizeFile, FileEventHook, NoHooks, PermitAll};
pub use records::{FileRecords, RecordError};

/// This service's migrations. **You** call `db.migrate(MIGRATIONS)`, when you
/// choose: a library that migrates your database on import has decided
/// something it does not know.
pub const MIGRATIONS: toolbox_db::EmbeddedMigrations = toolbox_db::embed_migrations!("migrations");

/// Start building the service.
///
/// # Arguments
///
/// * `records` - Where file identity is stored.
/// * `store` - Where the bytes go. Any `object_store` backend: local disk in
///   development, S3 in production.
#[must_use]
pub fn builder(records: Arc<dyn FileRecords>, store: Arc<dyn ObjectStore>) -> FileServiceBuilder {
    FileServiceBuilder::new(records, store)
}
