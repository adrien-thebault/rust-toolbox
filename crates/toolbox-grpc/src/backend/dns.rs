//! DNS discovery: a channel that re-resolves and rebalances.

use std::time::Duration;

use tonic::transport::{Channel, channel::Change};
use tracing::{info, warn};

use crate::{
    discovery::{EndpointSettings, resolve},
    error::GrpcError,
};

/// A channel that re-resolves and rebalances as addresses come and go.
///
/// # Arguments
///
/// * `name` - The backend's label, used when a resolution failure is logged.
/// * `host` - The DNS name to resolve. In a cluster it usually resolves to
///   several pods at once.
/// * `port` - The port every resolved address is reached on.
/// * `refresh` - How often to re-resolve. It is how quickly a rescheduled pod
///   is noticed, and how quickly a dead one is dropped.
/// * `settings` - The endpoint settings to apply to each resolved address,
///   carried explicitly because `Endpoint` has no getters to read them back
///   from.
pub(super) async fn dns_channel(
    name: &'static str,
    host: &str,
    port: u16,
    refresh: Duration,
    settings: EndpointSettings,
) -> Result<Channel, GrpcError> {
    let initial = resolve(host, port, settings).await?;
    let (channel, sender) = Channel::balance_channel::<usize>(initial.len().max(1));

    let mut known = initial.len();
    for (i, endpoint) in initial.into_iter().enumerate() {
        let _ = sender.send(Change::Insert(i, endpoint)).await;
    }

    // Re-resolution is what makes a scale-out visible. Without it the channel
    // holds whichever addresses resolved at startup, forever.
    let host = host.to_owned();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(refresh);
        ticker.tick().await;
        loop {
            ticker.tick().await;
            match resolve(&host, port, settings).await {
                Ok(resolved) => {
                    if resolved.len() != known {
                        info!(
                            backend = name,
                            from = known,
                            to = resolved.len(),
                            "backend address set changed"
                        );
                    }
                    let count = resolved.len();
                    for (i, endpoint) in resolved.into_iter().enumerate() {
                        if sender.send(Change::Insert(i, endpoint)).await.is_err() {
                            return; // the channel was dropped
                        }
                    }
                    for i in count..known {
                        let _ = sender.send(Change::Remove(i)).await;
                    }
                    known = count;
                }
                Err(e) => warn!(backend = name, error = %e, "re-resolution failed"),
            }
        }
    });

    Ok(channel)
}
