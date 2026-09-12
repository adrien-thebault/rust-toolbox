//! A ready-made liveness probe for a backend, over the standard
//! `grpc.health.v1.Health/Check` RPC.

use std::time::Duration;

use tonic_health::pb::{
    HealthCheckRequest, health_check_response::ServingStatus, health_client::HealthClient,
};
use toolbox_server::{Probe, poll_check};

use super::ClientChannel;

/// Poll `channel`'s standard `grpc.health.v1.Health/Check` every `interval`,
/// as a [`Probe`] to hand to `toolbox_server::ServerBuilder::check`.
///
/// The call carries whatever `channel` is configured with - shared secret,
/// deadline - the same as any other call on it, so this also proves the
/// secret is right when the peer's health service is gated with
/// [`crate::GrpcServerConfig::health_secret`].
///
/// # Arguments
///
/// * `channel` - The backend to probe.
/// * `name` - Reported in the health response and in the transition log.
/// * `interval` - How often to probe.
#[must_use]
pub fn poll_health(channel: ClientChannel, name: &'static str, interval: Duration) -> Probe {
    poll_check(name, interval, channel, |channel| async move {
        let mut client = HealthClient::new(channel.channel());
        client
            .check(HealthCheckRequest {
                service: String::new(),
            })
            .await
            .is_ok_and(|r| r.into_inner().status() == ServingStatus::Serving)
    })
}
