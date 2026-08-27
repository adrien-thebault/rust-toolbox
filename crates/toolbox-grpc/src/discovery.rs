//! Finding a backend's addresses, and keeping up when they change.
//!
//! `Channel::connect_lazy` on a DNS name opens exactly one HTTP/2 connection to
//! whichever address resolved first, and never looks again. A scale-out then
//! changes nothing, which is a silent failure - the deploy looks fine and the
//! load does not move.

use std::time::Duration;

use tonic::transport::{Channel, Endpoint, Uri};

use crate::error::GrpcError;

/// How to find a backend.
#[derive(Debug, Clone)]
pub enum Discovery {
    /// A fixed list, load-balanced across.
    Static(Vec<Uri>),
    /// A DNS name, re-resolved periodically and load-balanced across every
    /// address it returns.
    Dns {
        /// The hostname to resolve.
        name: String,
        /// The port every resolved address is reached on.
        port: u16,
        /// How often to re-resolve.
        refresh: Duration,
    },
    /// A single address that is itself a load balancer, so there is nothing to
    /// discover.
    Proxy(Uri),
}

impl Discovery {
    /// A single static address.
    ///
    /// # Arguments
    ///
    /// * `uri` - The address, `http://host:port`. Parsed now rather than at
    ///   connect time.
    ///
    /// # Errors
    /// [`GrpcError::Uri`] when `uri` does not parse.
    pub fn single(uri: &str) -> Result<Self, GrpcError> {
        Ok(Self::Static(vec![
            uri.parse().map_err(|_| GrpcError::Uri(uri.to_owned()))?,
        ]))
    }
}

/// The endpoint settings applied to every address, however it was discovered.
///
/// Carried explicitly rather than read back off a template `Endpoint`, because
/// `Endpoint` has no getters: a freshly resolved address would otherwise
/// silently take tonic's defaults instead of the configured timeouts, and one
/// rebalanced connection would behave differently from the rest.
#[derive(Debug, Clone, Copy)]
pub struct EndpointSettings {
    /// How long to wait for a connection.
    pub connect_timeout: Duration,
    /// How long a single call may take.
    pub request_timeout: Option<Duration>,
    /// HTTP/2 keepalive interval.
    pub keepalive: Option<Duration>,
}

impl EndpointSettings {
    /// Build an endpoint for `uri` with these settings.
    ///
    /// # Arguments
    ///
    /// * `uri` - The address to build for. Every setting on this struct is
    ///   applied to it, which is what keeps a freshly resolved address
    ///   configured like the first one.
    #[must_use]
    pub fn endpoint(&self, uri: Uri) -> Endpoint {
        let mut endpoint = Channel::builder(uri).connect_timeout(self.connect_timeout);
        if let Some(timeout) = self.request_timeout {
            endpoint = endpoint.timeout(timeout);
        }
        if let Some(interval) = self.keepalive {
            endpoint = endpoint
                .http2_keep_alive_interval(interval)
                .keep_alive_while_idle(true);
        }
        endpoint
    }
}

/// Resolve a DNS name to endpoints.
///
/// # Arguments
///
/// * `name` - The DNS name to look up.
/// * `port` - The port to attach to each resolved address, since DNS A records
///   carry none.
/// * `settings` - The settings to apply to every endpoint built from the
///   result.
pub(crate) async fn resolve(
    name: &str,
    port: u16,
    settings: EndpointSettings,
) -> Result<Vec<Endpoint>, GrpcError> {
    let addrs = tokio::net::lookup_host((name, port))
        .await
        .map_err(|e| GrpcError::Discovery(format!("resolving {name}:{port}: {e}")))?;

    let endpoints: Vec<Endpoint> = addrs
        .filter_map(|addr| format!("http://{addr}").parse::<Uri>().ok())
        .map(|uri| settings.endpoint(uri))
        .collect();

    if endpoints.is_empty() {
        return Err(GrpcError::Discovery(format!(
            "{name}:{port} resolved to no addresses"
        )));
    }
    Ok(endpoints)
}
