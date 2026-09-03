//! The Scalar docs page and the JSON spec route.

use axum::{Json, Router, routing::get};
use utoipa::openapi::OpenApi;
use utoipa_scalar::{Scalar, Servable as _};

use super::OpenApiConfig;

/// Serve the JSON spec and a docs page.
///
/// Scalar rather than Swagger UI: one JavaScript file against a directory of
/// assets, for a page that is read a few times a month. The spec is also
/// served raw at [`OpenApiConfig::spec_path`], which is what frontend codegen
/// fetches.
///
/// # Arguments
///
/// * `api` - The assembled spec. Run [`with_standard_errors`](super::with_standard_errors)
///   over it first, or it will claim the endpoints cannot fail.
/// * `cfg` - Where to mount the spec and the docs page.
pub fn openapi_router<S>(api: OpenApi, cfg: &OpenApiConfig) -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    let spec = api.clone();
    Router::new()
        .merge(Scalar::with_url(cfg.docs_path.clone(), api))
        .route(
            &cfg.spec_path,
            get(move || {
                let spec = spec.clone();
                async move { Json(spec) }
            }),
        )
}
