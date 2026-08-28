//! The gateway's HTTP surface.
//!
//! It owns authentication and speaks RFC 9457; the backend owns the data and
//! trusts its caller. That split is the whole architecture.

pub mod todo;

use axum::Router;
use toolbox_web::{
    ApiError,
    auth::{LoginLimit, auth_router, session_layer},
};

use crate::state::AppState;

/// The API surface, for the OpenAPI spec.
///
/// The login routes are deliberately absent: `auth_router`'s handlers are
/// private to `toolbox-web`, so a consumer documents them by hand or not at
/// all. Not at all, here.
#[derive(utoipa::OpenApi)]
#[openapi(
    paths(todo::list, todo::fetch, todo::create, todo::remove),
    components(schemas(todo::TodoDto, todo::NewTodo, todo::TodoPage)),
    info(title = "Todo API", version = "0.2.0")
)]
pub struct ApiDoc;

/// The spec, with the toolbox's standard error responses attached.
#[must_use]
pub fn openapi() -> utoipa::openapi::OpenApi {
    use utoipa::OpenApi as _;
    let mut api = ApiDoc::openapi();
    toolbox_web::openapi::with_standard_errors(&mut api);
    toolbox_web::openapi::bearer_security(&mut api);
    api
}

/// Every route the gateway serves, with the session middleware attached.
///
/// The state is taken here rather than left for the caller, because
/// `session_layer` needs it to build and axum only applies a layer to routes
/// already added.
///
/// # Arguments
///
/// * `state` - What the handlers and the auth routes read. It is cloned once
///   for the middleware and once for the router.
/// * `login` - How `/auth/login` and `/auth/refresh` are throttled. The rest of
///   the API is not, which is why the limiter goes inside `auth_router` rather
///   than over the whole gateway.
pub fn router(state: AppState, login: &LoginLimit) -> Router {
    Router::new()
        .merge(todo::router())
        .merge(auth_router::<AppState>(login))
        .layer(axum::middleware::from_fn_with_state(
            state.clone(),
            session_layer::<AppState>,
        ))
        .with_state(state)
}

/// Turn a backend's `Status` into the gateway's own problem document.
///
/// This is the seam that keeps `toolbox-web` free of tonic: the gRPC crate
/// decodes the `ErrorInfo`, and the web crate turns it into a response.
///
/// # Arguments
///
/// * `status` - What the backend returned. A status carrying no `ErrorInfo` -
///   a connection failure, say - keeps its gRPC code and loses only the
///   originating service's error code.
#[must_use]
pub fn from_backend(status: &tonic::Status) -> ApiError {
    let kind = toolbox_grpc::kind_for(status.code());
    toolbox_grpc::from_status(status).map_or_else(
        || ApiError::of_kind(kind, "Backend Error").with_detail(status.message().to_owned()),
        |info| ApiError::from_error_info(info, kind),
    )
}
