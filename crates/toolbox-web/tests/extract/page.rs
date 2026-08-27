use axum::{Router, routing::get};
use http::StatusCode;
use toolbox_core::{MAX_LIMIT, PageRequest};
use toolbox_web::extract::PageQuery;

use crate::{call, get as get_req};

fn app() -> Router {
    Router::new().route(
        "/items",
        get(|PageQuery(request): PageQuery| async move { format!("{request:?}") }),
    )
}

#[tokio::test]
async fn no_parameters_means_unpaged() {
    let (res, body) = call(app(), get_req("/items")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body.contains("Unpaged"), "{body}");
}

#[tokio::test]
async fn offset_and_limit_produce_a_bounded_window() {
    let (res, body) = call(app(), get_req("/items?offset=20&limit=10")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body.contains("offset: 20"), "{body}");
    assert!(body.contains("limit: 10"), "{body}");
}

#[tokio::test]
async fn supplying_only_one_defaults_the_other() {
    let (_, body) = call(app(), get_req("/items?limit=5")).await;
    assert!(body.contains("offset: 0"), "{body}");
    assert!(body.contains("limit: 5"), "{body}");
}

/// A silently clamped page is a caller that thinks it has all the data and
/// does not, so an over-large limit is refused rather than reduced.
#[tokio::test]
async fn a_limit_over_the_cap_is_refused_and_names_the_cap() {
    let (res, body) = call(app(), get_req(&format!("/items?limit={}", MAX_LIMIT + 1))).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);

    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["code"], "INVALID_PAGE");
    assert_eq!(v["metadata"]["max_limit"], MAX_LIMIT.to_string());
}

#[tokio::test]
async fn a_negative_offset_is_refused() {
    let (res, body) = call(app(), get_req("/items?offset=-1&limit=10")).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["code"], "INVALID_PAGE");
}

#[tokio::test]
async fn a_zero_limit_is_refused() {
    let (res, _) = call(app(), get_req("/items?limit=0")).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn a_sort_parameter_becomes_a_sort() {
    let (res, body) = call(app(), get_req("/items?sort=-created_at,title")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body.contains("created_at"), "{body}");
    assert!(body.contains("Desc"), "{body}");
}

#[tokio::test]
async fn a_malformed_sort_is_a_400() {
    let (res, body) = call(app(), get_req("/items?sort=title,")).await;
    assert_eq!(res.status(), StatusCode::BAD_REQUEST);
    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["code"], "INVALID_SORT");
}

#[test]
fn the_extractor_hands_back_the_request_it_built() {
    let q = PageQuery(PageRequest::unpaged(toolbox_core::Sort::unsorted()));
    assert!(q.request().offset().is_none());
    assert!(matches!(q.into_request(), PageRequest::Unpaged { .. }));
}
