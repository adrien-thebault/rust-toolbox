//! The HTTP gateway.
//!
//! It owns authentication, validation and the RFC 9457 error shape, and calls
//! the domains over gRPC. It holds no database of its own.

pub mod auth;
pub mod routes;
pub mod state;
