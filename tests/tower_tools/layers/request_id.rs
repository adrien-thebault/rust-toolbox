//! tests `src/tower_tools/layers/request_id.rs`: the layer pair assigns an
//! id, exposes it to the inner service, and echoes it onto the response.

use http::{Request, Response};
use rust_toolbox::tower_tools::layers::{
    RequestId, X_REQUEST_ID, propagate_request_id_layer, request_id_layer,
};
use std::convert::Infallible;
use tower::{ServiceBuilder, ServiceExt, service_fn};

/// the layer pair in its documented order - assignment outside, propagation
/// inside (propagation reads the header off the incoming request, so
/// assignment must already have run) - around a service that asserts what
/// it sees.
fn stack() -> impl tower::Service<
    Request<()>,
    Response = Response<()>,
    Error = Infallible,
    Future = impl Future<Output = Result<Response<()>, Infallible>>,
> {
    ServiceBuilder::new()
        .layer(request_id_layer())
        .layer(propagate_request_id_layer())
        .service(service_fn(|request: Request<()>| async move {
            assert!(
                request.extensions().get::<RequestId>().is_some(),
                "the inner service can read the id from extensions"
            );
            assert!(request.headers().contains_key(X_REQUEST_ID));
            Ok::<_, Infallible>(Response::new(()))
        }))
}

#[tokio::test]
async fn assigns_a_fresh_id_and_echoes_it_onto_the_response() {
    let response = stack()
        .oneshot(Request::builder().body(()).unwrap())
        .await
        .unwrap();

    let id = response
        .headers()
        .get(X_REQUEST_ID)
        .expect("the response carries the id")
        .to_str()
        .unwrap();
    assert!(!id.is_empty());
}

#[tokio::test]
async fn a_caller_provided_id_is_kept_and_echoed_back() {
    let response = stack()
        .oneshot(
            Request::builder()
                .header(X_REQUEST_ID, "caller-chosen")
                .body(())
                .unwrap(),
        )
        .await
        .unwrap();

    assert_eq!(
        response.headers().get(X_REQUEST_ID).unwrap(),
        "caller-chosen"
    );
}
