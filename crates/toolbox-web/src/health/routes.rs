//! `GET /health` (liveness) and `GET /ready` (readiness), and the body they
//! return.

use axum::{Json, Router, extract::State, routing::get};
use http::StatusCode;
use serde::Serialize;

use super::state::HealthState;

/// What `/health` and `/ready` return.
///
/// A plain `{status, checks}` body rather than `application/health+json`: that
/// draft expired and adoption is thin, so the standard is not worth the
/// coupling.
#[derive(Debug, Serialize)]
pub struct HealthResponse {
    /// `pass` or `fail`.
    pub status: &'static str,
    /// One entry per registered check.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<CheckResult>,
}

/// One dependency's result.
#[derive(Debug, Serialize)]
pub struct CheckResult {
    /// The check's name.
    pub name: &'static str,
    /// `pass` or `fail`.
    pub status: &'static str,
}

/// `GET /health` (liveness) and `GET /ready` (readiness).
///
/// Liveness answers whether the process is running at all, so it must not
/// consult a dependency: a database outage that fails liveness gets every
/// replica killed and restarted, which is strictly worse than serving errors.
pub fn health_router() -> Router<HealthState> {
    Router::new()
        .route("/health", get(health))
        .route("/ready", get(ready))
}

/// `GET /health`: whether the process is running at all. It consults no
/// dependency, on purpose.
#[allow(clippy::unused_async)]
async fn health() -> (StatusCode, Json<HealthResponse>) {
    (
        StatusCode::OK,
        Json(HealthResponse {
            status: "pass",
            checks: Vec::new(),
        }),
    )
}

/// `GET /ready`: whether this replica should receive traffic.
///
/// # Arguments
///
/// * `state` - The lifecycle handle: the drain state plus the registered
///   checks, combined into the one reading this route reports.
#[allow(clippy::unused_async)]
async fn ready(State(state): State<HealthState>) -> (StatusCode, Json<HealthResponse>) {
    let checks: Vec<CheckResult> = state
        .lifecycle
        .checks()
        .iter()
        .map(|c| CheckResult {
            name: c.name(),
            status: if c.is_ready() { "pass" } else { "fail" },
        })
        .collect();

    let ok = state.lifecycle.current().is_ready();
    let status = if ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        Json(HealthResponse {
            status: if ok { "pass" } else { "fail" },
            checks,
        }),
    )
}
