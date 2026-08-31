use std::convert::Infallible;

use http::{Request, Response};
use toolbox_grpc::{X_SHARED_SECRET, shared_secret_layer};
use tower::{Layer, ServiceExt, service_fn};

async fn ok(_req: Request<()>) -> Result<Response<()>, Infallible> {
    Ok(Response::new(()))
}

fn grpc_status(res: &Response<()>) -> Option<&str> {
    res.headers()
        .get("grpc-status")
        .and_then(|v| v.to_str().ok())
}

#[tokio::test]
async fn shared_secret_layer_rejects_a_missing_or_wrong_secret() {
    let svc = shared_secret_layer("s3cr3t").layer(service_fn(ok));

    let missing = svc.clone().oneshot(Request::new(())).await.unwrap();
    assert_eq!(grpc_status(&missing), Some("16"), "unauthenticated");

    let mut wrong = Request::new(());
    wrong
        .headers_mut()
        .insert(X_SHARED_SECRET, "nope".parse().unwrap());
    let wrong = svc.clone().oneshot(wrong).await.unwrap();
    assert_eq!(grpc_status(&wrong), Some("16"));

    let mut good = Request::new(());
    good.headers_mut()
        .insert(X_SHARED_SECRET, "s3cr3t".parse().unwrap());
    let good = svc.oneshot(good).await.unwrap();
    assert!(grpc_status(&good).is_none(), "the inner service answered");
}
