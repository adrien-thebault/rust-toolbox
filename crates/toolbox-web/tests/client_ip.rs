use std::net::{IpAddr, SocketAddr};

use http::HeaderMap;
use toolbox_web::client_ip::{TrustedHops, bucket, resolve_client_ip};

fn headers(pairs: &[&str]) -> HeaderMap {
    let mut h = HeaderMap::new();
    for v in pairs {
        h.append("x-forwarded-for", v.parse().unwrap());
    }
    h
}

fn peer() -> Option<SocketAddr> {
    Some("203.0.113.9:4444".parse().unwrap())
}

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

#[test]
fn one_proxy_appending_one_entry_is_the_default_case() {
    let h = headers(&["198.51.100.7"]);
    assert_eq!(
        resolve_client_ip(&h, peer(), TrustedHops::default()),
        Some(ip("198.51.100.7"))
    );
}

/// Counting from the right is what stops a client choosing its own bucket: a
/// forged leftmost entry is simply not the one read.
#[test]
fn a_forged_leading_entry_is_ignored() {
    let h = headers(&["1.1.1.1, 2.2.2.2, 198.51.100.7"]);
    assert_eq!(
        resolve_client_ip(&h, peer(), TrustedHops(1)),
        Some(ip("198.51.100.7")),
        "the entry the trusted proxy appended, not the one the client sent"
    );
}

/// The bug the review found in a naive code: `get()` reads only the
/// first header line, so a proxy that adds a second line is invisible.
#[test]
fn a_second_header_line_is_not_ignored() {
    let h = headers(&["1.1.1.1", "198.51.100.7"]);
    assert_eq!(
        resolve_client_ip(&h, peer(), TrustedHops(1)),
        Some(ip("198.51.100.7"))
    );
}

#[test]
fn a_cdn_in_front_of_a_load_balancer_needs_two_hops() {
    let h = headers(&["9.9.9.9, 198.51.100.7, 10.0.0.1"]);
    assert_eq!(
        resolve_client_ip(&h, peer(), TrustedHops(2)),
        Some(ip("198.51.100.7"))
    );
}

/// With nothing in front, no entry in the header is trustworthy at all.
#[test]
fn zero_hops_ignores_the_header_entirely() {
    let h = headers(&["1.1.1.1, 2.2.2.2"]);
    assert_eq!(
        resolve_client_ip(&h, peer(), TrustedHops(0)),
        Some(ip("203.0.113.9"))
    );
}

#[test]
fn a_list_shorter_than_the_hop_count_falls_back_to_the_peer() {
    let h = headers(&["198.51.100.7"]);
    assert_eq!(
        resolve_client_ip(&h, peer(), TrustedHops(3)),
        Some(ip("203.0.113.9")),
        "trusting whatever is there would be worse than admitting we do not know"
    );
}

#[test]
fn no_header_falls_back_to_the_peer() {
    assert_eq!(
        resolve_client_ip(&HeaderMap::new(), peer(), TrustedHops::default()),
        Some(ip("203.0.113.9"))
    );
}

#[test]
fn no_header_and_no_peer_is_none_rather_than_a_guess() {
    assert_eq!(
        resolve_client_ip(&HeaderMap::new(), None, TrustedHops::default()),
        None
    );
}

#[test]
fn entries_with_ports_and_brackets_parse() {
    assert_eq!(
        resolve_client_ip(&headers(&["198.51.100.7:1234"]), None, TrustedHops(1)),
        Some(ip("198.51.100.7"))
    );
    assert_eq!(
        resolve_client_ip(&headers(&["[2001:db8::1]:443"]), None, TrustedHops(1)),
        Some(ip("2001:db8::1"))
    );
    assert_eq!(
        resolve_client_ip(&headers(&["[2001:db8::1]"]), None, TrustedHops(1)),
        Some(ip("2001:db8::1"))
    );
}

#[test]
fn an_unparseable_entry_falls_back_rather_than_skipping_to_another() {
    let h = headers(&["198.51.100.7, garbage"]);
    assert_eq!(
        resolve_client_ip(&h, peer(), TrustedHops(1)),
        Some(ip("203.0.113.9")),
        "skipping to the next entry would let a client push the real one out of position"
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
