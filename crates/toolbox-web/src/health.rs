//! Liveness and readiness.
//!
//! A container orchestrator needs both endpoints and they must mean different
//! things - liveness answers "should I be killed?", readiness answers "should I
//! get traffic?" - and conflating them gets every replica restarted on a
//! dependency outage.

mod routes;
mod state;

pub use routes::{Check, HealthResponse, health_router};
pub use state::{HealthState, ReadinessCheck};
