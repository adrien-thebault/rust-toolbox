//! Who the caller actually is, at the IP level.
//!
//! "which entry of `X-Forwarded-For` do I trust?" is a decision with a wrong
//! answer, and it must be the *same* answer in the rate limiter, the access
//! log, the audit trail and any analytics. Four subsystems disagreeing about
//! who the caller was is its own class of bug.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use axum::extract::ConnectInfo;
use http::{Extensions, HeaderMap, HeaderName, request::Parts};
pub use ipnet::{IpNet, Ipv4Net, Ipv6Net};

/// The de-facto forwarded-client header.
pub const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");

/// Private proxy ranges, matching Caddy's `private_ranges` shortcut.
///
/// This is convenient for local/container deployments, but production should
/// prefer the exact proxy ranges when unrelated private-network workloads can
/// reach the server.
pub const PRIVATE_RANGES: [IpNet; 6] = [
    IpNet::V4(Ipv4Net::new_assert(Ipv4Addr::new(192, 168, 0, 0), 16)),
    IpNet::V4(Ipv4Net::new_assert(Ipv4Addr::new(172, 16, 0, 0), 12)),
    IpNet::V4(Ipv4Net::new_assert(Ipv4Addr::new(10, 0, 0, 0), 8)),
    IpNet::V4(Ipv4Net::new_assert(Ipv4Addr::new(127, 0, 0, 0), 8)),
    IpNet::V6(Ipv6Net::new_assert(
        Ipv6Addr::new(0xfd00, 0, 0, 0, 0, 0, 0, 0),
        8,
    )),
    IpNet::V6(Ipv6Net::new_assert(Ipv6Addr::LOCALHOST, 128)),
];

/// How to find the client's address behind proxies.
///
/// There is **no `Default`**: guessing turns a per-caller rate limit into a
/// global one, or lets a client forge its own bucket. Pick one deliberately,
/// and use the same one everywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientIpTrustPolicy {
    /// Nothing sits in front: use the TCP peer, ignore `X-Forwarded-For`.
    Peer,
    /// Walk `X-Forwarded-For` right to left, skipping entries in these networks
    /// (and a peer in them); the first entry outside is the client. Robust when
    /// the hop count varies - a CDN that is only sometimes in the path.
    BehindProxies(Vec<IpNet>),
}

/// The client IP for the configured trust model, falling back to the TCP peer.
///
/// Three things this gets right that the obvious implementation does not:
/// `get_all` rather than `get`, so a second `X-Forwarded-For:` line is not
/// ignored; never skipping past a malformed or unexpected entry, so a client
/// that sends its own header cannot push the real one out of position; and
/// falling back to the peer rather than trusting whatever is there when the
/// header is shorter than expected.
///
/// # Arguments
///
/// * `headers` - The request headers, read for `X-Forwarded-For`.
/// * `peer` - The TCP peer, used when there is no usable forwarded entry.
///   `None` when the router was not served with connect info.
/// * `trust` - How to read the header. See [`ClientIpTrustPolicy`].
#[must_use]
pub fn resolve_client_ip(
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
    trust: &ClientIpTrustPolicy,
) -> Option<IpAddr> {
    let peer_ip = peer.map(|p| p.ip());
    match trust {
        ClientIpTrustPolicy::BehindProxies(nets)
            if peer_ip.is_some_and(|ip| nets.iter().any(|net| net.contains(&ip))) =>
        {
            // Flatten every header line, since a proxy may add a second line
            // rather than extending the first. Any malformed line or entry
            // invalidates the forwarded chain instead of becoming a hole an
            // attacker could make the list close over.
            let forwarded = (|| {
                let mut entries = Vec::new();
                for value in headers.get_all(X_FORWARDED_FOR) {
                    let value = value.to_str().ok()?;
                    for entry in value.split(',').map(str::trim).filter(|s| !s.is_empty()) {
                        entries.push(parse_forwarded_entry(entry)?);
                    }
                }
                entries
                    .into_iter()
                    .rev()
                    .find(|ip| !nets.iter().any(|net| net.contains(ip)))
            })();
            forwarded.or(peer_ip)
        }
        ClientIpTrustPolicy::Peer | ClientIpTrustPolicy::BehindProxies(_) => peer_ip,
    }
}

/// Parse one `X-Forwarded-For` entry, which may carry a port or be bracketed.
///
/// # Arguments
///
/// * `entry` - One comma-separated entry. It may carry a port, and an IPv6
///   address may be bracketed, which is why this is not a bare `parse`.
fn parse_forwarded_entry(entry: &str) -> Option<IpAddr> {
    if let Ok(ip) = entry.parse::<IpAddr>() {
        return Some(ip);
    }
    if let Ok(addr) = entry.parse::<SocketAddr>() {
        return Some(addr.ip());
    }
    // `SocketAddr` handled `[::1]:8080`; accept the portless `[::1]` form too.
    entry.strip_prefix('[')?.strip_suffix(']')?.parse().ok()
}

/// The client IP of a request being extracted.
///
/// Needs the router to have been served with
/// `into_make_service_with_connect_info::<SocketAddr>()` for the peer fallback
/// to work; without it, a request with no `X-Forwarded-For` has no answer.
///
/// # Arguments
///
/// * `parts` - The request parts being extracted from.
/// * `trust` - How to read the forwarded header. See [`ClientIpTrustPolicy`].
#[must_use]
pub fn client_ip(parts: &Parts, trust: &ClientIpTrustPolicy) -> Option<IpAddr> {
    client_ip_of(&parts.headers, &parts.extensions, trust)
}

/// As [`client_ip`], from the pieces rather than from `Parts`.
///
/// A `tower` layer sees a whole `Request`, not `Parts`, so it cannot use
/// [`client_ip`] without taking the request apart.
///
/// # Arguments
///
/// * `headers` - The request headers.
/// * `extensions` - The request extensions, which is where axum puts the
///   connect info the peer fallback needs.
/// * `trust` - How to read the forwarded header.
#[must_use]
pub fn client_ip_of(
    headers: &HeaderMap,
    extensions: &Extensions,
    trust: &ClientIpTrustPolicy,
) -> Option<IpAddr> {
    let peer = extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
    resolve_client_ip(headers, peer, trust)
}

/// Bucket an address so a single client cannot occupy unbounded state.
///
/// A keyed limiter grows one entry per distinct key, and an attacker with an
/// IPv6 `/64` has 2^64 addresses to spend. Keying IPv6 by its `/64` prefix makes
/// that one entry, which is also the allocation unit an ISP hands out.
///
/// # Arguments
///
/// * `ip` - The address to bucket. IPv6 collapses to its /64, because an
///   attacker holding one has 2^64 addresses to spend against a keyed limiter.
#[must_use]
pub fn bucket(ip: IpAddr) -> IpAddr {
    match ip {
        IpAddr::V4(v4) => IpAddr::V4(v4),
        IpAddr::V6(v6) => {
            let mut octets = v6.octets();
            octets[8..].fill(0);
            IpAddr::V6(Ipv6Addr::from(octets))
        }
    }
}
