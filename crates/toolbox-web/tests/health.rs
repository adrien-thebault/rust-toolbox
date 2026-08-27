use axum::Router;
use http::StatusCode;
use toolbox_server::shutdown::Shutdown;
use toolbox_web::health::{HealthState, ReadinessCheck, health_router};

use crate::{call, get};

struct Db(bool);
impl ReadinessCheck for Db {
    fn name(&self) -> &'static str {
        "database"
    }
    fn is_ready(&self) -> bool {
        self.0
    }
}

fn app(shutdown: &Shutdown, checks: Vec<Box<dyn ReadinessCheck>>) -> Router {
    let state = HealthState::new(shutdown.readiness()).with_checks(checks);
    health_router().with_state(state)
}

#[tokio::test]
async fn health_and_ready_both_pass_when_everything_is_fine() {
    let shutdown = Shutdown::new();
    for path in ["/health", "/ready"] {
        let (res, body) = call(app(&shutdown, vec![]), get(path)).await;
        assert_eq!(res.status(), StatusCode::OK, "at {path}");
        assert!(body.contains("\"status\":\"pass\""), "{body}");
    }
}

/// The distinction that matters: readiness stops first, liveness never does,
/// because a failing liveness probe gets the pod killed rather than drained.
#[tokio::test]
async fn shutdown_fails_readiness_but_not_liveness() {
    let shutdown = Shutdown::new();
    shutdown.begin();

    let (ready, _) = call(app(&shutdown, vec![]), get("/ready")).await;
    assert_eq!(ready.status(), StatusCode::SERVICE_UNAVAILABLE);

    let (live, _) = call(app(&shutdown, vec![]), get("/health")).await;
    assert_eq!(
        live.status(),
        StatusCode::OK,
        "a draining pod is still alive"
    );
}

#[tokio::test]
async fn a_failing_dependency_fails_readiness_and_names_itself() {
    let shutdown = Shutdown::new();
    let (res, body) = call(app(&shutdown, vec![Box::new(Db(false))]), get("/ready")).await;
    assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);
    assert!(body.contains("database"), "{body}");
    assert!(body.contains("\"status\":\"fail\""), "{body}");
}

/// A database outage that fails liveness gets every replica killed and
/// restarted, which is strictly worse than serving errors.
#[tokio::test]
async fn liveness_does_not_consult_dependencies() {
    let shutdown = Shutdown::new();
    let (res, _) = call(app(&shutdown, vec![Box::new(Db(false))]), get("/health")).await;
    assert_eq!(res.status(), StatusCode::OK);
}

#[tokio::test]
async fn a_passing_dependency_is_reported_too() {
    let shutdown = Shutdown::new();
    let (res, body) = call(app(&shutdown, vec![Box::new(Db(true))]), get("/ready")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body.contains("database"), "{body}");
}
