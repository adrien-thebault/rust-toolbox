use toolbox_server::{stack::realtime_stack, trace_context::X_REQUEST_ID};
use tower::{Layer, ServiceExt};

use crate::{req, slow};

/// The whole reason `realtime_stack` exists: a stream must outlive the request
/// timeout that every other route wants.
#[tokio::test(start_paused = true)]
async fn the_realtime_stack_does_not_time_out() {
    let svc = realtime_stack().layer(tower::service_fn(slow));
    let res = svc.oneshot(req()).await.unwrap();
    assert_eq!(res.status(), http::StatusCode::OK);
    assert!(res.headers().contains_key(X_REQUEST_ID));
}
