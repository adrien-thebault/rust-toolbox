use axum::{Router, routing::get};
use toolbox_core::ErrorKind;
use toolbox_test::{TestApp, assert_problem, problem::ProblemResponse};
use toolbox_web::ApiError;

fn app() -> Router {
    Router::new()
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

#[test]
#[should_panic(expected = "code")]
fn assert_problem_rejects_the_wrong_code() {
    let response = ProblemResponse::new(404, "application/problem+json", r#"{"code":"OTHER"}"#);
    assert_problem!(response, 404, "EXPECTED");
}
