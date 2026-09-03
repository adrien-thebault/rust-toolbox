//! The facade root: every enabled crate is reachable under its short name.
//! Compiling is the test.

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
