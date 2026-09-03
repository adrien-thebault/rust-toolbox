use std::time::Duration;

use toolbox_server::{
    stack::{StackConfig, http_stack},
    trace_context::{TRACEPARENT, X_REQUEST_ID},
};
use tower::{Layer, ServiceExt};

use crate::{ok, req, slow};

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
