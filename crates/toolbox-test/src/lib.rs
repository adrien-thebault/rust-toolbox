//! The test harness.
//!
//! Spinning up a migrated throwaway database, a gateway and two gRPC backends
//! is thirty lines of setup that was near-identical in five `tests/common.rs`
//! files. **Dev-only**: nothing here should ever be a runtime dependency.

#[cfg(feature = "grpc")]
pub mod cluster;
#[cfg(feature = "db")]
pub mod db;
#[cfg(feature = "web")]
pub mod gateway;
pub mod problem;

#[cfg(feature = "grpc")]
pub use cluster::TestCluster;
#[cfg(feature = "db")]
pub use db::{migrated_conn, migrated_db, temp_db};
#[cfg(feature = "web")]
pub use gateway::{TEST_PEER, TestGateway};
