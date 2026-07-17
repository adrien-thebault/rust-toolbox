//! tests `src/tower_tools/layers/request_id.rs`: the layer pair assigns an
//! id, exposes it to the inner service, and echoes it onto the response.

use http::{Request, Response};
use rust_toolbox::tower_tools::layers::{
    CURRENT_REQUEST_ID, RequestId, X_REQUEST_ID, propagate_request_id_layer,
    request_id_context_layer, request_id_layer,
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

/// `request_id_context_layer` in its documented position - inside
/// `request_id_layer`, so the id it reads off `extensions()` is already
/// set - around a service that asserts it can read the same id back out of
/// `CURRENT_REQUEST_ID` ambient context, with no access to the original
/// request.
#[tokio::test]
async fn the_assigned_id_is_available_as_ambient_context_during_the_call() {
    let response = ServiceBuilder::new()
        .layer(request_id_layer())
        .layer(request_id_context_layer())
        .service(service_fn(|request: Request<()>| async move {
            let assigned = request
                .extensions()
                .get::<RequestId>()
                .unwrap()
                .header_value()
                .clone();
            let ambient = CURRENT_REQUEST_ID
                .try_with(|id| id.header_value().clone())
                .expect("the id is available as ambient context during the call");
            assert_eq!(ambient, assigned);
            Ok::<_, Infallible>(Response::new(()))
        }))
        .oneshot(Request::builder().body(()).unwrap())
        .await;

    assert!(response.is_ok());
}

/// outside of any `request_id_context_layer`-wrapped call, nothing is set
#[tokio::test]
async fn nothing_is_set_outside_the_call() {
    assert!(CURRENT_REQUEST_ID.try_with(|_| ()).is_err());
}

/// a request with no `RequestId` extension (i.e. `request_id_context_layer`
/// used without `request_id_layer` ahead of it) just leaves the context
/// unset, rather than failing the call
#[tokio::test]
async fn a_request_with_no_id_extension_leaves_the_context_unset() {
    let response = ServiceBuilder::new()
        .layer(request_id_context_layer())
        .service(service_fn(|_: Request<()>| async move {
            assert!(CURRENT_REQUEST_ID.try_with(|_| ()).is_err());
            Ok::<_, Infallible>(Response::new(()))
        }))
        .oneshot(Request::builder().body(()).unwrap())
        .await;

    assert!(response.is_ok());
}
