use std::time::Duration;

use toolbox_server::{
    stack::{StackConfig, grpc_stack, http_stack, realtime_stack},
    trace_context::{TRACEPARENT, X_REQUEST_ID},
};
use tower::{Layer, ServiceExt};

use crate::{ok, req, slow};

#[test]
fn the_defaults_are_the_ones_a_public_endpoint_needs() {
    let cfg = StackConfig::default();
    assert_eq!(cfg.timeout, Some(Duration::from_secs(30)));
    assert_eq!(cfg.max_body_bytes, Some(2 * 1024 * 1024));
}

#[tokio::test]
async fn the_http_stack_traces_every_response() {
    let svc = http_stack(StackConfig::default()).layer(tower::service_fn(ok));
    let res = svc.oneshot(req()).await.unwrap();
    assert_eq!(res.status(), http::StatusCode::OK);
    assert!(res.headers().contains_key(TRACEPARENT));
    assert!(res.headers().contains_key(X_REQUEST_ID));
}

/// A timeout that loses the request id is a timeout nobody can trace.
#[tokio::test(start_paused = true)]
async fn a_timed_out_request_still_carries_its_request_id() {
    let cfg = StackConfig::default().timeout(Some(Duration::from_millis(50)));
    let svc = http_stack(cfg).layer(tower::service_fn(slow));
    let res = svc.oneshot(req()).await.unwrap();
    assert_eq!(res.status(), http::StatusCode::GATEWAY_TIMEOUT);
    assert!(res.headers().contains_key(X_REQUEST_ID));
}

#[tokio::test(start_paused = true)]
async fn the_grpc_stack_reports_a_deadline_as_grpc_status_4() {
    let cfg = StackConfig::default().timeout(Some(Duration::from_millis(50)));
    let svc = grpc_stack(cfg).layer(tower::service_fn(slow));
    let res = svc.oneshot(req()).await.unwrap();
    assert_eq!(res.headers()["grpc-status"], "4");
}

/// The whole reason `realtime_stack` exists: a stream must outlive the request
/// timeout that every other route wants.
#[tokio::test(start_paused = true)]
async fn the_realtime_stack_does_not_time_out() {
    let svc = realtime_stack().layer(tower::service_fn(slow));
    let res = svc.oneshot(req()).await.unwrap();
    assert_eq!(res.status(), http::StatusCode::OK);
    assert!(res.headers().contains_key(X_REQUEST_ID));
}
