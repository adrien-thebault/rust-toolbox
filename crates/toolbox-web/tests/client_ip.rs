use std::net::{IpAddr, SocketAddr};

use axum::extract::ConnectInfo;
use http::{HeaderMap, HeaderValue, Request};
use toolbox_web::client_ip::{
    ClientIpTrustPolicy, IpNet, PRIVATE_RANGES, bucket, client_ip, client_ip_of, resolve_client_ip,
};

fn headers(pairs: &[&str]) -> HeaderMap {
    let mut h = HeaderMap::new();
    for v in pairs {
        h.append("x-forwarded-for", v.parse().unwrap());
    }
    h
}

fn public_peer() -> Option<SocketAddr> {
    Some("203.0.113.9:4444".parse().unwrap())
}

fn proxy_peer() -> Option<SocketAddr> {
    Some("10.0.0.2:4444".parse().unwrap())
}

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

fn net(s: &str) -> IpNet {
    s.parse().unwrap()
}

/// With nothing in front, no entry in the header is trustworthy at all.
#[test]
fn peer_trust_ignores_the_header_entirely() {
    let h = headers(&["1.1.1.1, 2.2.2.2"]);
    assert_eq!(
        resolve_client_ip(&h, public_peer(), &ClientIpTrustPolicy::Peer),
        Some(ip("203.0.113.9"))
    );
}

#[test]
fn an_untrusted_peer_cannot_supply_a_forwarded_address() {
    let trust = ClientIpTrustPolicy::BehindProxies(vec![net("10.0.0.0/8")]);
    let h = headers(&["1.1.1.1"]);
    assert_eq!(
        resolve_client_ip(&h, public_peer(), &trust),
        Some(ip("203.0.113.9")),
        "a direct caller must not choose its own rate-limit bucket"
    );
}

#[test]
fn no_header_falls_back_to_the_peer() {
    assert_eq!(
        resolve_client_ip(
            &HeaderMap::new(),
            proxy_peer(),
            &ClientIpTrustPolicy::BehindProxies(vec![net("10.0.0.0/8")])
        ),
        Some(ip("10.0.0.2"))
    );
}

#[test]
fn no_header_and_no_peer_is_none_rather_than_a_guess() {
    assert_eq!(
        resolve_client_ip(
            &HeaderMap::new(),
            None,
            &ClientIpTrustPolicy::BehindProxies(vec![net("10.0.0.0/8")])
        ),
        None
    );
}

#[test]
fn entries_with_ports_and_brackets_parse() {
    assert_eq!(
        resolve_client_ip(
            &headers(&["198.51.100.7:1234"]),
            proxy_peer(),
            &ClientIpTrustPolicy::BehindProxies(vec![net("10.0.0.0/8")])
        ),
        Some(ip("198.51.100.7"))
    );
    assert_eq!(
        resolve_client_ip(
            &headers(&["[2001:db8::1]:443"]),
            proxy_peer(),
            &ClientIpTrustPolicy::BehindProxies(vec![net("10.0.0.0/8")])
        ),
        Some(ip("2001:db8::1"))
    );
    assert_eq!(
        resolve_client_ip(
            &headers(&["[2001:db8::1]"]),
            proxy_peer(),
            &ClientIpTrustPolicy::BehindProxies(vec![net("10.0.0.0/8")])
        ),
        Some(ip("2001:db8::1"))
    );
}

#[test]
fn an_unparseable_entry_falls_back_rather_than_skipping_to_another() {
    let h = headers(&["198.51.100.7, garbage"]);
    assert_eq!(
        resolve_client_ip(
            &h,
            proxy_peer(),
            &ClientIpTrustPolicy::BehindProxies(vec![net("10.0.0.0/8")])
        ),
        Some(ip("10.0.0.2")),
        "skipping past a malformed entry would trust an incomplete chain"
    );
}

#[test]
fn a_non_text_header_falls_back_to_the_peer() {
    let mut h = HeaderMap::new();
    h.append(
        "x-forwarded-for",
        HeaderValue::from_bytes(b"\xff").expect("opaque header bytes are legal"),
    );
    assert_eq!(
        resolve_client_ip(
            &h,
            proxy_peer(),
            &ClientIpTrustPolicy::BehindProxies(vec![net("10.0.0.0/8")])
        ),
        Some(ip("10.0.0.2"))
    );
}

#[test]
fn bracketed_ipv6_rejects_trailing_junk() {
    assert_eq!(
        resolve_client_ip(
            &headers(&["[2001:db8::1]junk"]),
            proxy_peer(),
            &ClientIpTrustPolicy::BehindProxies(vec![net("10.0.0.0/8")])
        ),
        Some(ip("10.0.0.2"))
    );
}

/// `BehindProxies` is robust when the hop count varies: skip every entry in
/// the trusted networks, the first one outside is the client.
#[test]
fn behind_proxies_skips_the_trusted_ranges() {
    let trust = ClientIpTrustPolicy::BehindProxies(vec![net("10.0.0.0/8"), net("172.16.0.0/12")]);
    let h = headers(&["198.51.100.7, 172.16.4.4, 10.0.0.1"]);
    assert_eq!(
        resolve_client_ip(&h, proxy_peer(), &trust),
        Some(ip("198.51.100.7"))
    );
}

#[test]
fn behind_proxies_handles_a_shorter_chain_the_same_way() {
    let trust = ClientIpTrustPolicy::BehindProxies(vec![net("10.0.0.0/8")]);
    let h = headers(&["198.51.100.7, 10.9.9.9"]);
    assert_eq!(
        resolve_client_ip(&h, proxy_peer(), &trust),
        Some(ip("198.51.100.7"))
    );
}

#[test]
fn behind_proxies_falls_back_to_the_peer_when_every_entry_is_trusted() {
    let trust = ClientIpTrustPolicy::BehindProxies(vec![net("10.0.0.0/8")]);
    let h = headers(&["10.1.1.1, 10.2.2.2"]);
    assert_eq!(
        resolve_client_ip(&h, proxy_peer(), &trust),
        Some(ip("10.0.0.2"))
    );
}

#[test]
fn behind_proxies_stops_at_a_malformed_entry() {
    let trust = ClientIpTrustPolicy::BehindProxies(vec![net("10.0.0.0/8")]);
    let h = headers(&["198.51.100.7, garbage, 10.0.0.1"]);
    assert_eq!(
        resolve_client_ip(&h, proxy_peer(), &trust),
        Some(ip("10.0.0.2")),
        "a hole in the chain is not something to skip past"
    );
}

/// An attacker with a /64 otherwise has 2^64 keys to spend against a limiter
/// that grows one entry per key.
#[test]
fn ipv6_is_bucketed_by_its_64_prefix() {
    assert_eq!(bucket(ip("2001:db8:1:2:3:4:5:6")), ip("2001:db8:1:2::"));
    assert_eq!(
        bucket(ip("2001:db8:1:2::ffff")),
        bucket(ip("2001:db8:1:2:aaaa::1"))
    );
    assert_ne!(bucket(ip("2001:db8:1:2::1")), bucket(ip("2001:db8:1:3::1")));
}

#[test]
fn ipv4_is_kept_whole() {
    assert_eq!(bucket(ip("198.51.100.7")), ip("198.51.100.7"));
}

/// A tower layer sees a whole `Request`, so the peer fallback has to come out
/// of the `ConnectInfo` extension axum stores rather than a `SocketAddr`
/// argument. Wrong extension key and the fallback silently returns `None`.
#[test]
fn the_from_pieces_form_reads_the_peer_from_the_connect_info_extension() {
    let mut ext = http::Extensions::new();
    ext.insert(ConnectInfo(
        "203.0.113.9:4444".parse::<SocketAddr>().unwrap(),
    ));
    assert_eq!(
        client_ip_of(&HeaderMap::new(), &ext, &ClientIpTrustPolicy::Peer),
        Some(ip("203.0.113.9"))
    );
    assert_eq!(
        client_ip_of(
            &HeaderMap::new(),
            &http::Extensions::new(),
            &ClientIpTrustPolicy::Peer
        ),
        None,
        "no connect info means no peer to fall back to"
    );
}

#[test]
fn the_from_parts_form_resolves_the_same_way() {
    let (mut parts, ()) = Request::builder().uri("/").body(()).unwrap().into_parts();
    parts.headers = headers(&["198.51.100.7"]);
    parts
        .extensions
        .insert(ConnectInfo("10.0.0.2:4444".parse::<SocketAddr>().unwrap()));
    assert_eq!(
        client_ip(
            &parts,
            &ClientIpTrustPolicy::BehindProxies(PRIVATE_RANGES.to_vec())
        ),
        Some(ip("198.51.100.7"))
    );
}
