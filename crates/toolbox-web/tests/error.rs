use axum::{Router, routing::get};
use http::StatusCode;
use toolbox_core::{ErrorKind, PROBLEM_JSON, ServiceError};
use toolbox_web::ApiError;

use crate::{call, get as get_req};

#[derive(Debug, thiserror::Error)]
#[error("connection refused to 10.0.0.4:5432 (password=hunter2)")]
struct DbExploded;

impl ServiceError for DbExploded {
    fn code(&self) -> &'static str {
        "DB_UNAVAILABLE"
    }
    fn domain(&self) -> &'static str {
        "events"
    }
    fn kind(&self) -> ErrorKind {
        ErrorKind::Internal
    }
    fn metadata(&self) -> std::collections::BTreeMap<String, String> {
        std::collections::BTreeMap::from([("host".to_owned(), "10.0.0.4".to_owned())])
    }
}

#[derive(Debug, thiserror::Error)]
#[error("event 7 not found")]
struct NotFound;

impl ServiceError for NotFound {
    fn code(&self) -> &'static str {
        "EVENT_NOT_FOUND"
    }
    fn domain(&self) -> &'static str {
        "events"
    }
    fn kind(&self) -> ErrorKind {
        ErrorKind::NotFound
    }
}

fn app() -> Router {
    Router::new()
        .route(
            "/boom",
            get(|| async { Err::<(), ApiError>(DbExploded.into()) }),
        )
        .route(
            "/missing",
            get(|| async { Err::<(), ApiError>(NotFound.into()) }),
        )
        .route(
            "/limited",
            get(|| async {
                Err::<(), ApiError>(
                    ApiError::of_kind(ErrorKind::ResourceExhausted, "Too Many Requests")
                        .with_retry_after(30),
                )
            }),
        )
}

/// The bug this test exists for: five documents claimed RFC 7807 while the
/// code served `application/json`.
#[tokio::test]
async fn every_error_is_served_as_problem_json() {
    for path in ["/boom", "/missing", "/limited"] {
        let (res, _) = call(app(), get_req(path)).await;
        assert_eq!(res.headers()["content-type"], PROBLEM_JSON, "at {path}");
    }
}

#[tokio::test]
async fn a_4xx_body_is_the_exact_problem_document() {
    let (res, body) = call(app(), get_req("/missing")).await;
    assert_eq!(res.status(), StatusCode::NOT_FOUND);

    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["type"], "about:blank");
    assert_eq!(v["title"], "Not Found");
    assert_eq!(v["status"], 404);
    assert_eq!(v["code"], "EVENT_NOT_FOUND");
    assert_eq!(v["domain"], "events");
    assert_eq!(v["detail"], "event 7 not found");
}

/// The other bug: raw database text, including a password, reaching an
/// anonymous caller.
#[tokio::test]
async fn a_5xx_body_discloses_nothing_but_the_code() {
    let (res, body) = call(app(), get_req("/boom")).await;
    assert_eq!(res.status(), StatusCode::INTERNAL_SERVER_ERROR);

    assert!(!body.contains("10.0.0.4"), "{body}");
    assert!(!body.contains("hunter2"), "{body}");
    assert!(!body.contains("connection refused"), "{body}");

    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert!(
        v.get("detail").is_none(),
        "detail is cleared on 5xx: {body}"
    );
    assert!(
        v.get("metadata").is_none(),
        "metadata is cleared on 5xx: {body}"
    );
    assert_eq!(
        v["code"], "DB_UNAVAILABLE",
        "the stable code survives, so support can act"
    );
    assert_eq!(v["status"], 500);
}

/// The limiter knows how long to wait, and a naive code discarded it.
#[tokio::test]
async fn a_429_says_when_to_come_back() {
    let (res, _) = call(app(), get_req("/limited")).await;
    assert_eq!(res.status(), StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(res.headers()["retry-after"], "30");
}

#[test]
fn every_kind_maps_to_the_documented_status() {
    use toolbox_web::status_for;
    assert_eq!(status_for(ErrorKind::NotFound), StatusCode::NOT_FOUND);
    assert_eq!(
        status_for(ErrorKind::InvalidArgument),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(
        status_for(ErrorKind::Unauthenticated),
        StatusCode::UNAUTHORIZED
    );
    assert_eq!(
        status_for(ErrorKind::PermissionDenied),
        StatusCode::FORBIDDEN
    );
    assert_eq!(status_for(ErrorKind::Conflict), StatusCode::CONFLICT);
    assert_eq!(
        status_for(ErrorKind::ResourceExhausted),
        StatusCode::TOO_MANY_REQUESTS
    );
    assert_eq!(
        status_for(ErrorKind::FailedPrecondition),
        StatusCode::PRECONDITION_FAILED
    );
    assert_eq!(status_for(ErrorKind::Timeout), StatusCode::GATEWAY_TIMEOUT);
    assert_eq!(
        status_for(ErrorKind::Unavailable),
        StatusCode::SERVICE_UNAVAILABLE
    );
    assert_eq!(
        status_for(ErrorKind::Unimplemented),
        StatusCode::NOT_IMPLEMENTED
    );
    assert_eq!(
        status_for(ErrorKind::Internal),
        StatusCode::INTERNAL_SERVER_ERROR
    );
}

#[tokio::test]
async fn an_error_info_from_grpc_becomes_the_same_problem_shape() {
    use toolbox_core::ErrorInfo;
    let info = ErrorInfo::new("BACKEND_SAID_NO", "events").with("id", "7");
    let app = Router::new().route(
        "/proxied",
        get(move || async move {
            Err::<(), ApiError>(ApiError::from_error_info(info, ErrorKind::Conflict))
        }),
    );

    let (res, body) = call(app, get_req("/proxied")).await;
    assert_eq!(res.status(), StatusCode::CONFLICT);
    assert_eq!(res.headers()["content-type"], PROBLEM_JSON);

    let v: serde_json::Value = serde_json::from_str(&body).unwrap();
    assert_eq!(v["code"], "BACKEND_SAID_NO");
    assert_eq!(v["metadata"]["id"], "7");
}
