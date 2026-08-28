//! One dependency line for the toolbox.
//!
//! A small project wants one entry in its manifest and one version to bump. A
//! large one should depend on the individual crates directly, because that
//! compiles less.
//!
//! ```toml
//! toolbox = { git = "...", tag = "v0.2.0", features = ["db", "web"] }
//! ```
//!
//! Each feature pulls in the crate of the same name. The dependency order is
//! `core -> db -> cluster -> server -> {web, grpc}`, so enabling `web` also
//! enables `server`, `cluster`, `auth` and `core`.

#[cfg(feature = "auth")]
pub use toolbox_auth as auth;
#[cfg(feature = "cluster")]
pub use toolbox_cluster as cluster;
#[cfg(feature = "core")]
pub use toolbox_core as core;
#[cfg(feature = "db")]
pub use toolbox_db as db;
#[cfg(feature = "grpc")]
pub use toolbox_grpc as grpc;
#[cfg(feature = "db")]
pub use toolbox_macros as macros;
#[cfg(feature = "server")]
pub use toolbox_server as server;
#[cfg(feature = "web")]
pub use toolbox_web as web;

pub mod deps;
pub mod prelude;
