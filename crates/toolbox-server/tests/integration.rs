//! One harness per crate; the module tree mirrors `src/`.
#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

mod deadline;
mod lifecycle;
mod stack;
mod telemetry;
mod trace_context;

use std::convert::Infallible;

use bytes::Bytes;
use http::{Request, Response};
use http_body_util::Full;
use toolbox_server::{StartupConfig, bind};

/// A body type that satisfies every bound the stacks impose.
pub type TestBody = Full<Bytes>;

/// A handler that answers 200 immediately.
pub async fn ok(_req: Request<TestBody>) -> Result<Response<TestBody>, Infallible> {
    Ok(Response::new(Full::new(Bytes::from_static(b"ok"))))
}

/// A handler that sleeps before answering.
pub async fn slow(_req: Request<TestBody>) -> Result<Response<TestBody>, Infallible> {
    tokio::time::sleep(std::time::Duration::from_secs(10)).await;
    Ok(Response::new(Full::new(Bytes::from_static(b"slow"))))
}

/// An empty request.
pub fn req() -> Request<TestBody> {
    Request::builder().uri("/x").body(Full::default()).unwrap()
}

#[tokio::test]
async fn binding_hands_out_a_listener_on_the_requested_address() {
    let cfg = StartupConfig::new("127.0.0.1:0".parse().unwrap());
    let listener = bind(&cfg).await.unwrap();
    assert!(listener.local_addr().unwrap().port() > 0);
}
