//! Liveness and readiness.
//!
//! A container orchestrator needs both endpoints and they must mean different
//! things - liveness answers "should I be killed?", readiness answers "should I
//! get traffic?". Neither template had either, which means a deployment had no
//! way to know when a pod was up.

use std::sync::Arc;

use axum::{Router, extract::State, routing::get};
use http::StatusCode;
use serde::Serialize;
use toolbox_server::shutdown::ReadinessHandle;

/// A dependency whose health decides whether this process should get traffic.
pub trait ReadinessCheck: Send + Sync + 'static {
    /// What to call it in the response body.
    fn name(&self) -> &'static str;
    /// Whether it is currently usable.
    fn is_ready(&self) -> bool;
}

/// What `/health` and `/ready` return.
///
/// A plain `{status, checks}` body rather than `application/health+json`: that
/// draft expired and adoption is thin, so the standard is not worth the
/// coupling.
#[derive(Debug, Serialize)]
pub struct Health {
    /// `pass` or `fail`.
    pub status: &'static str,
    /// One entry per registered check.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub checks: Vec<Check>,
}

/// One dependency's result.
#[derive(Debug, Serialize)]
pub struct Check {
    /// The check's name.
    pub name: &'static str,
    /// `pass` or `fail`.
    pub status: &'static str,
}

/// The state `health_router` needs.
#[derive(Clone)]
pub struct HealthState {
    readiness: ReadinessHandle,
    checks: Arc<Vec<Box<dyn ReadinessCheck>>>,
}

impl std::fmt::Debug for HealthState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HealthState")
            .field("checks", &self.checks.len())
            .finish_non_exhaustive()
    }
}

impl HealthState {
    /// A state reporting ready until shutdown begins.
    ///
    /// # Arguments
    ///
    /// * `readiness` - The handle the drain flips, which is what makes `/ready`
    ///   fail before the listener closes.
    #[must_use]
    pub fn new(readiness: ReadinessHandle) -> Self {
        Self {
            readiness,
            checks: Arc::new(Vec::new()),
        }
    }

    /// Register the dependencies readiness depends on.
    ///
    /// # Arguments
    ///
    /// * `checks` - The dependencies readiness consults. Liveness never does,
    ///   because a database outage that fails liveness restarts every replica.
    #[must_use]
    pub fn with_checks(mut self, checks: Vec<Box<dyn ReadinessCheck>>) -> Self {
        self.checks = Arc::new(checks);
        self
    }
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
async fn health() -> (StatusCode, axum::Json<Health>) {
    (
        StatusCode::OK,
        axum::Json(Health {
            status: "pass",
            checks: Vec::new(),
        }),
    )
}

/// `GET /ready`: whether this replica should receive traffic.
///
/// # Arguments
///
/// * `state` - The readiness handle and the registered checks.
#[allow(clippy::unused_async)]
async fn ready(State(state): State<HealthState>) -> (StatusCode, axum::Json<Health>) {
    let checks: Vec<Check> = state
        .checks
        .iter()
        .map(|c| Check {
            name: c.name(),
            status: if c.is_ready() { "pass" } else { "fail" },
        })
        .collect();

    let ok = state.readiness.is_ready() && checks.iter().all(|c| c.status == "pass");
    let status = if ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (
        status,
        axum::Json(Health {
            status: if ok { "pass" } else { "fail" },
            checks,
        }),
    )
}
