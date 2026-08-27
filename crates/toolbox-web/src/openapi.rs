//! OpenAPI generation.
//!
//! It bridges `utoipa` and this crate's error shape, so every operation
//! documents the same problem responses without any handler hand-annotating
//! them.
//!
//! This crate ships the **spec**, never a client. Turning the spec into
//! TypeScript is one `npx openapi-typescript` invocation in whatever
//! repository owns the frontend.

use axum::Router;
use utoipa::openapi::{OpenApi, RefOr, Response, ResponseBuilder};
use utoipa_scalar::{Scalar, Servable as _};

/// Where the docs page and the spec are mounted.
#[derive(Debug, Clone)]
pub struct DocsConfig {
    /// The path serving the JSON spec.
    pub spec_path: String,
    /// The path serving the human-readable page.
    pub docs_path: String,
}

impl Default for DocsConfig {
    fn default() -> Self {
        Self {
            spec_path: "/openapi.json".to_owned(),
            docs_path: "/docs".to_owned(),
        }
    }
}

/// Serve a spec and a docs page.
///
/// Scalar rather than Swagger UI: one JavaScript file against a directory of
/// assets, for a page that is read a few times a month.
///
/// # Arguments
///
/// * `api` - The assembled spec. Run [`with_standard_errors`] over it first, or
///   it will claim the endpoints cannot fail.
/// * `cfg` - Where to mount the spec and the docs page.
pub fn openapi_router<S>(api: OpenApi, cfg: &DocsConfig) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new().merge(Scalar::with_url(cfg.docs_path.clone(), api))
}

/// The status codes every operation can produce, whether or not it says so.
const STANDARD_ERRORS: &[(&str, &str)] = &[
    ("400", "The request was malformed or failed validation"),
    ("401", "No credentials, or credentials that did not verify"),
    ("403", "Authenticated, but not permitted"),
    ("404", "No such resource"),
    ("409", "The request conflicts with current state"),
    ("429", "Rate limited; see Retry-After"),
    (
        "500",
        "An internal failure. The body carries a code and a request id, never a detail",
    ),
];

/// Attach the standard error responses to every operation.
///
/// Without this, each handler hand-annotates seven responses, which means in
/// practice that most annotate none and the spec claims endpoints cannot fail.
///
/// # Arguments
///
/// * `api` - The spec to amend in place. Every operation gains the responses it
///   can actually produce.
pub fn with_standard_errors(api: &mut OpenApi) {
    for item in api.paths.paths.values_mut() {
        // PathItem holds one Option per HTTP method rather than a map, so the
        // operations are enumerated rather than iterated.
        let operations = [
            item.get.as_mut(),
            item.put.as_mut(),
            item.post.as_mut(),
            item.delete.as_mut(),
            item.options.as_mut(),
            item.head.as_mut(),
            item.patch.as_mut(),
            item.trace.as_mut(),
        ];
        for operation in operations.into_iter().flatten() {
            for (status, description) in STANDARD_ERRORS {
                if operation.responses.responses.contains_key(*status) {
                    continue;
                }
                operation
                    .responses
                    .responses
                    .insert((*status).to_owned(), problem_response(description));
            }
        }
    }
}

/// One standard error response, referencing the shared problem schema rather
/// than repeating it.
///
/// # Arguments
///
/// * `description` - What this status means, as it appears in the docs page.
fn problem_response(description: &str) -> RefOr<Response> {
    RefOr::T(
        ResponseBuilder::new()
            .description(description)
            .content(
                toolbox_core::PROBLEM_JSON,
                utoipa::openapi::ContentBuilder::new().build(),
            )
            .build(),
    )
}

/// Declare bearer-token security on the whole document.
///
/// # Arguments
///
/// * `api` - The spec to amend in place. Without this the docs page has no way
///   to send a token.
pub fn bearer_security(api: &mut OpenApi) {
    use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};

    let components = api.components.get_or_insert_with(Default::default);
    components.add_security_scheme(
        "bearer",
        SecurityScheme::Http(
            HttpBuilder::new()
                .scheme(HttpAuthScheme::Bearer)
                .bearer_format("JWT")
                .build(),
        ),
    );
}

/// Serialize a spec with **stable key ordering**.
///
/// This is what makes the drift guard work at all: `git diff --exit-code` is
/// useless if key order shuffles between runs, and it will, because some of
/// the maps underneath are hash-ordered.
///
/// # Arguments
///
/// * `api` - The spec to serialize. Key order is made stable here, which is
///   what lets CI diff the committed file.
///
/// # Errors
/// [`serde_json::Error`] when the document cannot be serialized, which would
/// mean utoipa produced something invalid.
pub fn dump_openapi(api: &OpenApi) -> Result<String, serde_json::Error> {
    let value = serde_json::to_value(api)?;
    serde_json::to_string_pretty(&canonicalize(value))
}

/// Rebuild every object with its keys sorted.
///
/// Done explicitly rather than relying on `serde_json::Map` being a `BTreeMap`,
/// because that depends on whether anything in the workspace enabled
/// `serde_json`'s `preserve_order` feature - and feature unification means that
/// is not this crate's decision to make.
///
/// # Arguments
///
/// * `value` - The document to rebuild with sorted keys. Done explicitly rather
///   than relying on `serde_json::Map` being a `BTreeMap`, because that depends
///   on a feature any crate in the graph can turn off.
fn canonicalize(value: serde_json::Value) -> serde_json::Value {
    match value {
        serde_json::Value::Object(map) => {
            let mut sorted: std::collections::BTreeMap<String, serde_json::Value> =
                std::collections::BTreeMap::new();
            for (k, v) in map {
                sorted.insert(k, canonicalize(v));
            }
            serde_json::Value::Object(sorted.into_iter().collect())
        }
        serde_json::Value::Array(items) => {
            serde_json::Value::Array(items.into_iter().map(canonicalize).collect())
        }
        other => other,
    }
}
