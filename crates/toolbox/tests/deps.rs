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
