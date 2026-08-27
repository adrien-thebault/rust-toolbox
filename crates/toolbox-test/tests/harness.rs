use axum::{Router, routing::get};
use diesel::sqlite::SqliteConnection;
use toolbox_core::ErrorKind;
use toolbox_test::{TestApp, assert_problem, problem::ProblemResponse, temp_db};
use toolbox_web::ApiError;

#[tokio::test]
async fn a_temp_db_is_private_to_the_test_that_made_it() {
    let (a, guard_a) = temp_db::<SqliteConnection>();
    let (b, guard_b) = temp_db::<SqliteConnection>();
    assert_ne!(
        guard_a.path(),
        guard_b.path(),
        "two tests must not share a file"
    );
    assert!(guard_a.path().exists());

    // Both are usable.
    a.query(|_c: &mut SqliteConnection| Ok(1_i32))
        .await
        .unwrap();
    b.query(|_c: &mut SqliteConnection| Ok(1_i32))
        .await
        .unwrap();
}

#[test]
fn a_temp_db_deletes_itself() {
    let path = {
        let (_db, guard) = temp_db::<SqliteConnection>();
        let path = guard.path();
        assert!(path.exists());
        path
    };
    assert!(!path.exists(), "the file went away with the guard");
}

fn app() -> Router {
    Router::new()
        .route("/ok", get(|| async { "fine" }))
        .route(
            "/missing",
            get(|| async {
                Err::<(), ApiError>(
                    ApiError::not_found("no such event").with_code("EVENT_NOT_FOUND"),
                )
            }),
        )
        .route(
            "/invalid",
            get(|| async {
                Err::<(), ApiError>(
                    ApiError::of_kind(ErrorKind::InvalidArgument, "Invalid Argument")
                        .with_code("VALIDATION_FAILED")
                        .with_metadata("email", "not an email"),
                )
            }),
        )
}

#[tokio::test]
async fn the_gateway_runs_in_process_with_no_port_and_no_readiness_wait() {
    let app = TestApp::new(app());
    let res = app.get("/ok").await;
    assert_eq!(res.status_code(), 200);
    assert_eq!(res.text(), "fine");
}

#[tokio::test]
async fn assert_problem_checks_status_code_and_media_type() {
    let app = TestApp::new(app());
    let problem = app.get_problem("/missing").await;
    assert_problem!(problem, 404, "EVENT_NOT_FOUND");
}

#[tokio::test]
async fn assert_problem_can_require_a_metadata_field() {
    let app = TestApp::new(app());
    let problem = app.get_problem("/invalid").await;
    assert_problem!(problem, 400, "VALIDATION_FAILED", "email");
}

/// The assertion exists to catch a body that is JSON but not problem+json,
/// which is the bug the whole error shape was rewritten for.
#[test]
#[should_panic(expected = "problem+json")]
fn assert_problem_rejects_a_plain_json_error_body() {
    let response = ProblemResponse::new(404, "application/json", r#"{"code":"X"}"#);
    assert_problem!(response, 404, "X");
}

#[tokio::test]
async fn post_json_reaches_a_post_route() {
    let app = TestApp::new(Router::new().route(
        "/echo",
        axum::routing::post(|body: String| async move { body }),
    ));
    let res = app.post_json("/echo", &serde_json::json!({"a": 1})).await;
    assert_eq!(res.status_code(), 200);
    assert!(res.text().contains("\"a\":1"), "{}", res.text());
}

#[test]
#[should_panic(expected = "code")]
fn assert_problem_rejects_the_wrong_code() {
    let response = ProblemResponse::new(404, "application/problem+json", r#"{"code":"OTHER"}"#);
    assert_problem!(response, 404, "EXPECTED");
}
