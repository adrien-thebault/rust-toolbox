//! The gRPC services this domain exposes.
//!
//! One file each, named after the proto service it implements (without the
//! redundant `_service` suffix). They share the crate's schema, migrations and
//! pool, which is the whole reason a domain is a crate and a service is not.

pub mod todo;

pub use todo::{TodoService, TodoServiceError};
