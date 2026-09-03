//! OpenAPI generation.
//!
//! It bridges `utoipa` and this crate's error shape, so every operation
//! documents the same problem responses without any handler hand-annotating
//! them.
//!
//! This crate ships the **spec**, never a client. Turning the spec into
//! TypeScript is one `npx openapi-typescript` invocation in whatever
//! repository owns the frontend.

mod augment;
mod dump;
mod router;

pub use augment::{bearer_security, with_standard_errors};
pub use dump::dump_openapi;
pub use router::openapi_router;

/// Where the docs page and the JSON spec are mounted.
#[derive(Debug, Clone)]
pub struct OpenApiConfig {
    /// The path serving the JSON spec, for frontend tooling.
    pub spec_path: String,
    /// The path serving the human-readable page.
    pub docs_path: String,
}

impl Default for OpenApiConfig {
    fn default() -> Self {
        Self {
            spec_path: "/openapi.json".to_owned(),
            docs_path: "/docs".to_owned(),
        }
    }
}
