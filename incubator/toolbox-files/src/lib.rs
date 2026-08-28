//! File policy, ingest and serving semantics.
//!
//! It encodes four decisions that are otherwise re-made per project and got
//! wrong in at least one of them - content-addressed keys, sniffing rather than
//! trusting the declared type, security headers on every download, and
//! enforcing the size cap as bytes flow rather than after buffering.
//!
//! **There is no `FileStore` trait here.** `object_store::ObjectStore` already
//! is that trait, with local, S3, GCS, Azure, in-memory and signed-URL
//! backends, maintained by somebody else. This crate is the layer above it.
//!
//! Nothing in the storage half returns an axum type, so a gRPC file service
//! can use the same decisions for its metadata.
//!
//! # Parked
//!
//! This crate is in `incubator/`, outside the workspace. It is the one part of
//! the toolbox with a domain of its own - a file has an owner, a quota, a
//! declared type - and it was the sole reason `toolbox-web` carried a feature
//! that pulled in `toolbox-grpc`. See `incubator/README.md`.

pub mod error;
pub mod ingest;
pub mod meta;
pub mod policy;
#[cfg(feature = "service")]
pub mod proto;
pub mod serve;
#[cfg(feature = "service")]
pub mod service;
pub mod stream;
#[cfg(feature = "web")]
pub mod web;

pub use error::FileError;
pub use ingest::{ingest, sniff};
pub use meta::{FILE_PREFIX, FileMeta, Ingested, key_for};
pub use policy::{MimePolicy, Quota, UploadPolicy};
#[cfg(feature = "service")]
pub use proto::{CHUNK_SIZE, upload_chunk, upload_info};
pub use serve::{ByteRange, Conditionals, ServeDecision, parse_range, serve};
pub use stream::{ingest_stream, read_stream};
