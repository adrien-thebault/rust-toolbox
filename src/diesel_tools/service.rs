/// mod for the pool-backed, tonic-server-convertible `DatabaseService` trait
mod database_service;
pub use database_service::*;

/// mod for the higher-level `EntityService` trait
mod entity_service;
pub use entity_service::*;
