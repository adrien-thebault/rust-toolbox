//! Who the caller actually is, at the IP level.
//!
//! "which entry of `X-Forwarded-For` do I trust?" is a decision with a wrong
//! answer, and it must be the *same* answer in the rate limiter, the access
//! log, the audit trail and any analytics. Four subsystems disagreeing about
//! who the caller was is its own class of bug.

use std::{
    net::{IpAddr, SocketAddr},
    num::NonZeroUsize,
};

use axum::extract::ConnectInfo;
use http::{HeaderMap, HeaderName, request::Parts};
pub use ipnet::IpNet;

/// The de-facto forwarded-client header.
pub const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");

/// How to find the client's address behind proxies.
///
/// There is **no `Default`**: guessing turns a per-caller rate limit into a
/// global one, or lets a client forge its own bucket. Pick one deliberately,
/// and use the same one everywhere.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientIpTrust {
    /// Nothing sits in front: use the TCP peer, ignore `X-Forwarded-For`.
    Peer,
    /// Exactly `n` proxies append one entry each, so the client is `n` entries
    /// from the right. `Hops(1)` is a plain Caddy or nginx in front.
    Hops(NonZeroUsize),
    /// Walk `X-Forwarded-For` right to left, skipping entries in these networks
    /// (and a peer in them); the first entry outside is the client. Robust when
    /// the hop count varies - a CDN that is only sometimes in the path.
    BehindProxies(Vec<IpNet>),
}

impl ClientIpTrust {
    /// `n` proxy hops, or [`Peer`](Self::Peer) when `n` is zero.
    ///
    /// # Arguments
    ///
    /// * `n` - How many proxies append to `X-Forwarded-For`. Zero means nothing
    ///   is in front and the header is not read at all.
    #[must_use]
    pub fn hops(n: usize) -> Self {
        NonZeroUsize::new(n).map_or(Self::Peer, Self::Hops)
    }
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
/// * `trust` - How to read the header. See [`ClientIpTrust`].
#[must_use]
pub fn resolve_client_ip(
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
    trust: &ClientIpTrust,
) -> Option<IpAddr> {
    let peer_ip = peer.map(|p| p.ip());
    if *trust == ClientIpTrust::Peer {
        return peer_ip;
    }

    // Flatten every header line, since a proxy may add a second line rather
    // than extending the first. Parsed up front so a malformed entry is a
    // `None` in place, not a hole the list closes over.
    let entries: Vec<Option<IpAddr>> = headers
        .get_all(X_FORWARDED_FOR)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(parse_forwarded_entry)
        .collect();

    match trust {
        ClientIpTrust::Peer => peer_ip,
        ClientIpTrust::Hops(n) => entries
            .len()
            .checked_sub(n.get())
            .and_then(|i| entries.get(i).copied().flatten())
            .or(peer_ip),
        ClientIpTrust::BehindProxies(nets) => {
            for entry in entries.iter().rev() {
                match entry {
                    // A trusted proxy: keep walking left.
                    Some(ip) if nets.iter().any(|net| net.contains(ip)) => {}
                    // The first address outside the trusted set is the client.
                    Some(ip) => return Some(*ip),
                    // A malformed entry: stop rather than skip past it.
                    None => break,
                }
            }
            peer_ip
        }
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
    // `[::1]:8080` and `[::1]` forms.
    let inner = entry.strip_prefix('[')?;
    let end = inner.find(']')?;
    inner[..end].parse().ok()
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
/// * `trust` - How to read the forwarded header. See [`ClientIpTrust`].
#[must_use]
pub fn client_ip(parts: &Parts, trust: &ClientIpTrust) -> Option<IpAddr> {
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
    extensions: &http::Extensions,
    trust: &ClientIpTrust,
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
            IpAddr::V6(std::net::Ipv6Addr::from(octets))
        }
    }
}
