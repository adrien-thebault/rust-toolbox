//! The facade's contract is that the names resolve. Compiling is the test.

#[test]
fn every_enabled_crate_is_reachable_under_its_short_name() {
    // If a feature stops wiring its crate through, these stop compiling.
    let _ = toolbox::core::ErrorKind::NotFound;
    let _ = toolbox::db::DbError::Conflict;
    let _ = toolbox::cluster::Deployment::Single;
    let _: toolbox::server::StackConfig = toolbox::server::StackConfig::default();
    let _ = toolbox::auth::Principal::new("u", "local");
    let _ = toolbox::web::status_for(toolbox::core::ErrorKind::NotFound);
    let _ = toolbox::grpc::code_for(toolbox::core::ErrorKind::NotFound);
}

#[test]
fn the_prelude_carries_the_names_a_handler_actually_uses() {
    use toolbox::prelude::*;

    let _ = ErrorKind::NotFound;
    let _: PageRequest = PageRequest::unpaged(Sort::unsorted());
    let _ = Problem::new(404, "Not Found");
    let _ = DbError::NotFound;
    let _ = Deployment::Single;
    let _ = ApiError::not_found("x");
    let _ = Principal::new("u", "local");
}

/// The rule this exists to enforce: re-export crates whose types you hand out,
/// never crates whose macros consumers invoke.
#[test]
fn deps_re_exports_the_crates_whose_types_cross_the_boundary() {
    let _: toolbox::deps::http::StatusCode = toolbox::deps::http::StatusCode::OK;
    let _ = toolbox::deps::axum::Router::<()>::new();
    let _ = toolbox::deps::tonic::Code::Ok;
    let _: toolbox::deps::tower_http::cors::CorsLayer =
        toolbox::deps::tower_http::cors::CorsLayer::new();
}
