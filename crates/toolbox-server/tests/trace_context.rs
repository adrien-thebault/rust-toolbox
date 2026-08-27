use toolbox_server::trace_context::{
    CURRENT_TRACE, TRACEPARENT, TraceContext, TraceContextLayer, X_REQUEST_ID, current_request_id,
};
use tower::{Layer, Service, ServiceExt};

use crate::{TestBody, ok, req};

const PARENT: &str = "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01";

#[test]
fn traceparent_round_trips() {
    let ctx = TraceContext::parse(PARENT).unwrap();
    assert_eq!(ctx.trace_id(), "4bf92f3577b34da6a3ce929d0e0e4736");
    assert_eq!(ctx.span_id(), "00f067aa0ba902b7");
    assert!(ctx.is_sampled());
    assert_eq!(ctx.to_string(), PARENT);
}

#[test]
fn a_malformed_traceparent_is_rejected_rather_than_propagated() {
    for bad in [
        "",
        "00",
        "01-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01", // unknown version
        "00-4bf92f3577b34da6a3ce929d0e0e473-00f067aa0ba902b7-01",  // short trace id
        "00-4BF92F3577B34DA6A3CE929D0E0E4736-00f067aa0ba902b7-01", // uppercase
        "00-00000000000000000000000000000000-00f067aa0ba902b7-01", // all-zero trace id
        "00-4bf92f3577b34da6a3ce929d0e0e4736-0000000000000000-01", // all-zero span id
        "00-4bf92f3577b34da6a3ce929d0e0e4736-00f067aa0ba902b7-01-extra",
    ] {
        assert!(TraceContext::parse(bad).is_none(), "should reject `{bad}`");
    }
}

#[test]
fn a_child_keeps_the_trace_and_takes_a_fresh_span() {
    let parent = TraceContext::parse(PARENT).unwrap();
    let child = parent.child();
    assert_eq!(child.trace_id(), parent.trace_id());
    assert_ne!(child.span_id(), parent.span_id());
}

#[test]
fn a_minted_root_is_well_formed() {
    let ctx = TraceContext::new_root();
    assert_eq!(TraceContext::parse(&ctx.to_string()).unwrap(), ctx);
}

#[tokio::test]
async fn an_incoming_traceparent_keeps_its_trace_id() {
    let svc = TraceContextLayer::new().layer(tower::service_fn(ok));
    let request = http::Request::builder()
        .uri("/x")
        .header(TRACEPARENT, PARENT)
        .body(TestBody::default())
        .unwrap();

    let res = svc.oneshot(request).await.unwrap();
    let out = TraceContext::parse(res.headers()[TRACEPARENT].to_str().unwrap()).unwrap();
    assert_eq!(out.trace_id(), "4bf92f3577b34da6a3ce929d0e0e4736");
    assert_eq!(
        res.headers()[X_REQUEST_ID],
        "4bf92f3577b34da6a3ce929d0e0e4736"
    );
}

#[tokio::test]
async fn a_missing_traceparent_is_minted_and_echoed() {
    let svc = TraceContextLayer::new().layer(tower::service_fn(ok));
    let res = svc.oneshot(req()).await.unwrap();

    let out = TraceContext::parse(res.headers()[TRACEPARENT].to_str().unwrap()).unwrap();
    assert_eq!(res.headers()[X_REQUEST_ID], out.trace_id());
}

#[tokio::test]
async fn the_context_is_visible_to_the_handler() {
    async fn handler(
        _req: http::Request<TestBody>,
    ) -> Result<http::Response<TestBody>, std::convert::Infallible> {
        let id = current_request_id().expect("a request id is always in scope");
        assert!(CURRENT_TRACE.try_with(|_| ()).is_ok());
        Ok(http::Response::builder()
            .header("x-seen", id)
            .body(TestBody::default())
            .unwrap())
    }

    let mut svc = TraceContextLayer::new().layer(tower::service_fn(handler));
    let res = svc.ready().await.unwrap().call(req()).await.unwrap();
    assert_eq!(res.headers()["x-seen"], res.headers()[X_REQUEST_ID]);
}

#[test]
fn there_is_no_request_id_outside_a_request() {
    assert!(current_request_id().is_none());
}
