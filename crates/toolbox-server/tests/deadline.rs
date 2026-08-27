use std::time::Duration;

use toolbox_server::deadline::{
    DeadlineLayer, GRPC_TIMEOUT, format_grpc_timeout, parse_grpc_timeout, time_remaining,
};
use tower::{Layer, ServiceExt};

use crate::{TestBody, req, slow};

#[test]
fn grpc_timeout_units_are_the_wire_spec_ones() {
    assert_eq!(parse_grpc_timeout("10S"), Some(Duration::from_secs(10)));
    assert_eq!(parse_grpc_timeout("500m"), Some(Duration::from_millis(500)));
    assert_eq!(parse_grpc_timeout("2M"), Some(Duration::from_secs(120)));
    assert_eq!(parse_grpc_timeout("1H"), Some(Duration::from_secs(3600)));
    assert_eq!(parse_grpc_timeout("250u"), Some(Duration::from_micros(250)));
    assert_eq!(parse_grpc_timeout("7n"), Some(Duration::from_nanos(7)));
}

#[test]
fn a_malformed_grpc_timeout_is_ignored_rather_than_guessed_at() {
    for bad in ["", "S", "10", "10X", "-5S", "abcS"] {
        assert_eq!(parse_grpc_timeout(bad), None, "should reject `{bad}`");
    }
}

#[test]
fn grpc_timeout_formats_as_milliseconds() {
    assert_eq!(format_grpc_timeout(Duration::from_secs(3)), "3000m");
}

#[tokio::test(start_paused = true)]
async fn a_handler_that_overruns_the_deadline_gets_a_504() {
    let svc = DeadlineLayer::new(Some(Duration::from_millis(50))).layer(tower::service_fn(slow));
    let res = svc.oneshot(req()).await.unwrap();
    assert_eq!(res.status(), http::StatusCode::GATEWAY_TIMEOUT);
}

#[tokio::test(start_paused = true)]
async fn a_grpc_deadline_is_reported_as_grpc_status_4_not_as_a_504() {
    let svc = DeadlineLayer::new(Some(Duration::from_millis(50)))
        .grpc()
        .layer(tower::service_fn(slow));
    let res = svc.oneshot(req()).await.unwrap();
    assert_eq!(res.status(), http::StatusCode::OK);
    assert_eq!(res.headers()["grpc-status"], "4");
}

#[tokio::test(start_paused = true)]
async fn a_caller_may_ask_for_less_time_but_never_for_more() {
    // The caller asks for 10ms against a 10s default: the caller wins.
    let svc = DeadlineLayer::new(Some(Duration::from_secs(10))).layer(tower::service_fn(slow));
    let request = http::Request::builder()
        .uri("/x")
        .header(GRPC_TIMEOUT, "10m")
        .body(TestBody::default())
        .unwrap();
    let res = svc.oneshot(request).await.unwrap();
    assert_eq!(res.status(), http::StatusCode::GATEWAY_TIMEOUT);
}

#[tokio::test]
async fn the_remaining_budget_is_visible_to_the_handler() {
    async fn handler(
        _req: http::Request<TestBody>,
    ) -> Result<http::Response<TestBody>, std::convert::Infallible> {
        let left = time_remaining().expect("a deadline is in scope");
        assert!(
            left <= Duration::from_secs(5) && left > Duration::from_secs(4),
            "{left:?}"
        );
        Ok(http::Response::new(TestBody::default()))
    }

    let svc = DeadlineLayer::new(Some(Duration::from_secs(5))).layer(tower::service_fn(handler));
    svc.oneshot(req()).await.unwrap();
}

#[tokio::test]
async fn without_a_deadline_the_handler_still_runs() {
    let svc = DeadlineLayer::new(None).layer(tower::service_fn(crate::ok));
    let res = svc.oneshot(req()).await.unwrap();
    assert_eq!(res.status(), http::StatusCode::OK);
}
