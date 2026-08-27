//! Who the caller actually is, at the IP level.
//!
//! "which entry of `X-Forwarded-For` do I trust?" is a decision with a wrong
//! answer, and it must be the *same* answer in the rate limiter, the access
//! log, the audit trail and any analytics. Four subsystems disagreeing about
//! who the caller was is its own class of bug.

use std::net::{IpAddr, SocketAddr};

use axum::extract::ConnectInfo;
use http::{HeaderMap, HeaderName, request::Parts};

/// The de-facto forwarded-client header.
pub const X_FORWARDED_FOR: HeaderName = HeaderName::from_static("x-forwarded-for");

/// How many proxies append to `X-Forwarded-For` before the request arrives.
///
/// The trustworthy entry is at `len - trusted_hops`; "rightmost" is just the
/// `trusted_hops = 1` case.
///
/// - `1` (the default): one reverse proxy appended one entry, so the last
///   entry is the one it observed. Correct for a plain Caddy or nginx in front.
/// - `0`: **ignore the header entirely** and use the TCP peer. Correct when
///   nothing sits in front, where no entry is trustworthy at all.
/// - `2` or more: a CDN in front of a load balancer. Note that the entry you
///   then trust is the CDN's egress IP, so every user behind one point of
///   presence shares a bucket.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrustedHops(pub usize);

impl Default for TrustedHops {
    fn default() -> Self {
        Self(1)
    }
}

/// The client IP, from the entry `trusted_hops` back, falling back to the TCP
/// peer.
///
/// Three things this gets right that the obvious implementation does not:
/// `get_all` rather than `get`, so a second `X-Forwarded-For:` line is not
/// ignored; counting from the right, so a client that sends its own header
/// cannot choose its own bucket; and falling back to the peer when the list is
/// shorter than the configured hop count, rather than trusting whatever is
/// there.
///
/// # Arguments
///
/// * `headers` - The request headers, read for `X-Forwarded-For`.
/// * `peer` - The TCP peer, used when there is no usable forwarded entry.
///   `None` when the router was not served with connect info.
/// * `hops` - How many proxies append before the request arrives. The
///   trustworthy entry is that far from the right.
#[must_use]
pub fn resolve_client_ip(
    headers: &HeaderMap,
    peer: Option<SocketAddr>,
    hops: TrustedHops,
) -> Option<IpAddr> {
    if hops.0 == 0 {
        return peer.map(|p| p.ip());
    }

    // Flatten every header line, since a proxy may add a second line rather
    // than extending the first.
    let entries: Vec<&str> = headers
        .get_all(X_FORWARDED_FOR)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(','))
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();

    entries
        .len()
        .checked_sub(hops.0)
        .and_then(|i| entries.get(i))
        .and_then(|s| parse_forwarded_entry(s))
        .or_else(|| peer.map(|p| p.ip()))
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
/// * `hops` - How many proxies to trust.
#[must_use]
pub fn client_ip(parts: &Parts, hops: TrustedHops) -> Option<IpAddr> {
    client_ip_of(&parts.headers, &parts.extensions, hops)
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
/// * `hops` - How many proxies to trust.
#[must_use]
pub fn client_ip_of(
    headers: &HeaderMap,
    extensions: &http::Extensions,
    hops: TrustedHops,
) -> Option<IpAddr> {
    let peer = extensions.get::<ConnectInfo<SocketAddr>>().map(|c| c.0);
    resolve_client_ip(headers, peer, hops)
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
