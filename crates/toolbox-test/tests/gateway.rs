use axum::{Router, routing::get};
use toolbox_test::TestGateway;

fn app() -> Router {
    Router::new().route("/ok", get(|| async { "fine" }))
}

#[tokio::test]
async fn the_gateway_runs_in_process_with_no_port_and_no_readiness_wait() {
    let app = TestGateway::new(app());
    let res = app.get("/ok").await;
    assert_eq!(res.status_code(), 200);
    assert_eq!(res.text(), "fine");
}

#[tokio::test]
async fn post_json_reaches_a_post_route() {
    let app = TestGateway::new(Router::new().route(
        "/echo",
        axum::routing::post(|body: String| async move { body }),
    ));
    let res = app.post_json("/echo", &serde_json::json!({"a": 1})).await;
    assert_eq!(res.status_code(), 200);
    assert!(res.text().contains("\"a\":1"), "{}", res.text());
}
