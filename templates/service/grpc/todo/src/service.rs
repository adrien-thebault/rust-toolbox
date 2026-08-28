//! The gRPC services this domain exposes.
//!
//! One file each, named after the proto service it implements. They share the
//! crate's schema, migrations and pool, which is the whole reason a domain is a
//! crate and a service is not.

pub mod todo_service;

pub use todo_service::{TodoService, TodoServiceError};
