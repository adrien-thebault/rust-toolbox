//! Axum-specific application of the shared server stacks.

use axum::{Router, extract::DefaultBodyLimit};
use toolbox_server::{HttpStack, RealtimeStack, StackConfig};

/// Apply the standard HTTP middleware and request-body limit to a router.
pub fn apply_http_stack(router: Router, config: StackConfig) -> Router {
    let router = match config.max_body_bytes {
        Some(max) => router.layer(DefaultBodyLimit::max(max)),
        None => router.layer(DefaultBodyLimit::disable()),
    };
    router.layer(HttpStack::new(config))
}

/// Apply the long-lived HTTP middleware with no request-body limit.
pub fn apply_realtime_stack(router: Router) -> Router {
    router
        .layer(DefaultBodyLimit::disable())
        .layer(RealtimeStack::new())
}
