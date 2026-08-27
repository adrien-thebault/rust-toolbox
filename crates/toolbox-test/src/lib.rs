//! The test harness.
//!
//! Spinning up a migrated throwaway database, a gateway and two gRPC backends
//! is thirty lines of setup that was near-identical in five `tests/common.rs`
//! files. **Dev-only**: nothing here should ever be a runtime dependency.

#[cfg(feature = "web")]
pub mod app;
#[cfg(feature = "grpc")]
pub mod cluster;
#[cfg(feature = "db")]
pub mod db;
pub mod problem;

#[cfg(feature = "web")]
pub use app::{TEST_PEER, TestApp};
#[cfg(feature = "grpc")]
pub use cluster::{BackendAddrs, TestCluster};
#[cfg(feature = "db")]
pub use db::temp_db;
