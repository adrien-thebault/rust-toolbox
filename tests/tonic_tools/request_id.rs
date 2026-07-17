//! tests `src/tonic_tools/request_id.rs`: `request_id_interceptor` stamps
//! the ambient `CURRENT_REQUEST_ID` (if any) onto outgoing gRPC metadata.

use http::HeaderValue;
use rust_toolbox::tonic_tools::request_id_interceptor;
use rust_toolbox::tower_tools::layers::{CURRENT_REQUEST_ID, RequestId};
use tonic::Request;

#[tokio::test]
async fn stamps_the_ambient_id_onto_the_outgoing_request() {
    let id = RequestId::new(HeaderValue::from_static("some-request-id"));
    let req = CURRENT_REQUEST_ID
        .scope(id, async { request_id_interceptor(Request::new(())) })
        .await
        .unwrap();

    assert_eq!(
        req.metadata()
            .get("x-request-id")
            .expect("the id is stamped onto the outgoing metadata")
            .to_str()
            .unwrap(),
        "some-request-id"
    );
}

#[tokio::test]
async fn leaves_the_request_untouched_with_no_ambient_id() {
    let req = request_id_interceptor(Request::new(())).unwrap();
    assert!(req.metadata().get("x-request-id").is_none());
}
