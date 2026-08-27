use axum::{Router, routing::get};
use http::StatusCode;
use toolbox_auth::{Principal, Role};
use toolbox_web::extract::{Authenticated, MaybeAuthenticated};

use crate::{call, get as get_req};

/// A consumer defines its own roles; the toolbox never ships an `Admin`.
struct Admin;
impl Role for Admin {
    const NAME: &'static str = "ADMIN";
}

fn app() -> Router {
    Router::new()
        .route("/admin", get(|_: Authenticated<Admin>| async { "ok" }))
        .route("/any", get(|a: Authenticated| async move { a.0.subject }))
        .route(
            "/maybe",
            get(|m: MaybeAuthenticated| async move {
                m.0.map_or_else(|| "anonymous".to_owned(), |p| p.subject)
            }),
        )
}

fn as_principal(app: Router, principal: Principal) -> Router {
    app.layer(axum::Extension(principal.clone()))
        .layer(axum::middleware::from_fn(
            move |mut req: axum::extract::Request, next: axum::middleware::Next| {
                let principal = principal.clone();
                async move {
                    req.extensions_mut().insert(principal);
                    next.run(req).await
                }
            },
        ))
}

#[tokio::test]
async fn an_admin_reaches_an_admin_route() {
    let app = as_principal(app(), Principal::new("u1", "local").with_role("ADMIN"));
    let (res, body) = call(app, get_req("/admin")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body, "ok");
}

/// The check is in the signature, so it cannot be forgotten and it is visible
/// in the route table.
#[tokio::test]
async fn a_non_admin_is_refused_with_a_problem_document() {
    let app = as_principal(app(), Principal::new("u1", "local").with_role("USER"));
    let (res, body) = call(app, get_req("/admin")).await;

    assert_eq!(res.status(), StatusCode::FORBIDDEN);
    assert_eq!(res.headers()["content-type"], toolbox_core::PROBLEM_JSON);

    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["code"], "FORBIDDEN");
    assert_eq!(
        v["metadata"]["required_role"], "ADMIN",
        "the body says which role was needed"
    );
}

#[tokio::test]
async fn an_anonymous_caller_gets_401_not_403() {
    let (res, body) = call(app(), get_req("/admin")).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["code"], "UNAUTHENTICATED");
}

#[tokio::test]
async fn any_authenticated_caller_passes_the_default_role() {
    let app = as_principal(app(), Principal::new("u42", "local"));
    let (res, body) = call(app, get_req("/any")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body, "u42");
}

#[tokio::test]
async fn an_optional_principal_is_none_when_anonymous() {
    let (res, body) = call(app(), get_req("/maybe")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body, "anonymous");
}

#[test]
fn a_principal_is_keyed_by_issuer_and_subject_together() {
    // A subject is unique only within its issuer, so two providers can each
    // have a user `1`.
    let a = Principal::new("1", "keycloak");
    let b = Principal::new("1", "google");
    assert_ne!(a.key(), b.key());
}

#[test]
fn role_checks_work_dynamically_too() {
    let p = Principal::new("u", "local").with_roles(["ADMIN", "EDITOR"]);
    assert!(p.has_role("EDITOR"));
    assert!(!p.has_role("OWNER"));
    assert!(p.require_role("ADMIN").is_ok());
    assert!(p.require_role("OWNER").is_err());
    assert!(p.has::<Admin>());
}
