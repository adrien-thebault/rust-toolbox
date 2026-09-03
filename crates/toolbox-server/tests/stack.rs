//! `StackConfig`; the per-transport stacks are in the submodules.

use std::time::Duration;

use toolbox_server::stack::StackConfig;

mod grpc;
mod http;
mod realtime;

#[test]
fn the_defaults_are_the_ones_a_public_endpoint_needs() {
    let cfg = StackConfig::default();
    assert_eq!(cfg.timeout, Some(Duration::from_secs(30)));
    assert_eq!(cfg.max_body_bytes, Some(2 * 1024 * 1024));
}
