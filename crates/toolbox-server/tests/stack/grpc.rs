use std::time::Duration;

use toolbox_server::stack::{StackConfig, grpc_stack};
use tower::{Layer, ServiceExt};

use crate::{req, slow};

#[tokio::test(start_paused = true)]
async fn the_grpc_stack_reports_a_deadline_as_grpc_status_4() {
    let cfg = StackConfig::default().timeout(Some(Duration::from_millis(50)));
    let svc = grpc_stack(cfg).layer(tower::service_fn(slow));
    let res = svc.oneshot(req()).await.unwrap();
    assert_eq!(res.headers()["grpc-status"], "4");
}
