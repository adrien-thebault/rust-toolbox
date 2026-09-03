use axum::body::Body;
use http::{Request, StatusCode};
use toolbox_web::openapi::{OpenApiConfig, openapi_router};
use utoipa::OpenApi;

use super::Api;
use crate::call;

fn app() -> axum::Router {
    openapi_router::<()>(Api::openapi(), &OpenApiConfig::default()).with_state(())
}

#[tokio::test]
async fn the_raw_spec_is_served_as_json_for_tooling() {
    let req = Request::builder()
        .uri("/openapi.json")
        .body(Body::empty())
        .unwrap();
    let (res, body) = call(app(), req).await;
    assert_eq!(res.status(), StatusCode::OK);
    let spec: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(spec["paths"]["/api/todos"].is_object(), "{body}");
}

#[tokio::test]
async fn the_docs_page_is_served() {
    let req = Request::builder().uri("/docs").body(Body::empty()).unwrap();
    let (res, _) = call(app(), req).await;
    assert_eq!(res.status(), StatusCode::OK);
}
