use std::net::IpAddr;

use ipnet::IpNet;
use toolbox_auth::{
    AuthError, ForwardedHeaders, ForwardedIdentity, ForwardedIdentityProvider, parse_network,
};

fn ip(s: &str) -> IpAddr {
    s.parse().unwrap()
}

fn provider() -> ForwardedIdentityProvider {
    ForwardedIdentityProvider::new(&["10.0.0.0/8", "192.168.1.5"]).unwrap()
}

fn forwarded(peer: &str) -> ForwardedIdentity {
    ForwardedIdentity {
        user: Some("ada".to_owned()),
        groups: Some("admins, staff".to_owned()),
        email: Some("ada@example.test".to_owned()),
        peer: Some(ip(peer)),
    }
}

/// A spoofable header is total authentication bypass, so there is no default
/// and no "trust everything in development" mode - that mode is what reaches
/// production.
/// Different proxies use different header names; the default is oauth2-proxy's
/// and the presets cover Authelia and the `X-Auth-Request-*` mode.
#[test]
fn the_header_set_is_configurable() {
    assert_eq!(provider().headers().user, "x-forwarded-user");

    let authelia = provider().with_headers(ForwardedHeaders::authelia());
    assert_eq!(authelia.headers().user, "remote-user");
    assert_eq!(authelia.headers().groups, "remote-groups");

    let alt = provider().with_headers(ForwardedHeaders::x_auth_request());
    assert_eq!(alt.headers().email, "x-auth-request-email");
}

#[test]
fn it_refuses_to_construct_without_a_trusted_proxy_list() {
    let err = ForwardedIdentityProvider::new(&[]).unwrap_err();
    assert!(matches!(err, AuthError::Malformed(_)));
    assert!(
        err.to_string().contains("bypass"),
        "the message says why: {err}"
    );
}

#[test]
fn a_malformed_cidr_is_refused_at_construction() {
    assert!(ForwardedIdentityProvider::new(&["not-an-ip"]).is_err());
    assert!(ForwardedIdentityProvider::new(&["10.0.0.0/99"]).is_err());
}

#[test]
fn a_trusted_peer_can_assert_an_identity() {
    let principal = provider().principal(&forwarded("10.1.2.3")).unwrap();
    assert_eq!(principal.subject, "ada");
    assert!(principal.has_role("ADMINS"));
    assert!(principal.has_role("STAFF"));
    assert_eq!(principal.email.as_deref(), Some("ada@example.test"));
}

/// The property the whole provider rests on.
#[test]
fn an_untrusted_peer_cannot() {
    let err = provider().principal(&forwarded("203.0.113.9")).unwrap_err();
    assert_eq!(err, AuthError::Unauthenticated);
}

/// No peer means no way to know whether the headers came from the proxy.
#[test]
fn no_peer_address_is_refused() {
    let mut identity = forwarded("10.0.0.1");
    identity.peer = None;
    assert_eq!(
        provider().principal(&identity).unwrap_err(),
        AuthError::Unauthenticated
    );
}

#[test]
fn a_trusted_peer_asserting_no_user_is_refused() {
    let mut identity = forwarded("10.0.0.1");
    identity.user = None;
    assert_eq!(
        provider().principal(&identity).unwrap_err(),
        AuthError::Unauthenticated
    );

    identity.user = Some(String::new());
    assert_eq!(
        provider().principal(&identity).unwrap_err(),
        AuthError::Unauthenticated
    );
}

#[test]
fn an_exact_address_is_a_host_route_not_a_network() {
    let provider = ForwardedIdentityProvider::new(&["192.168.1.5"]).unwrap();
    assert!(provider.trusts(ip("192.168.1.5")));
    assert!(!provider.trusts(ip("192.168.1.6")));
}

#[test]
fn cidr_matching_respects_the_prefix_length() {
    let eight: IpNet = parse_network("10.0.0.0/8").unwrap();
    assert!(eight.contains(&ip("10.255.255.255")));
    assert!(!eight.contains(&ip("11.0.0.1")));

    let twenty_four = parse_network("192.168.1.0/24").unwrap();
    assert!(twenty_four.contains(&ip("192.168.1.200")));
    assert!(!twenty_four.contains(&ip("192.168.2.1")));

    let odd = parse_network("10.0.0.0/12").unwrap();
    assert!(odd.contains(&ip("10.15.0.1")));
    assert!(!odd.contains(&ip("10.16.0.1")));
}

#[test]
fn ipv6_networks_match_too() {
    let net = parse_network("2001:db8::/32").unwrap();
    assert!(net.contains(&ip("2001:db8:1234::1")));
    assert!(!net.contains(&ip("2001:db9::1")));
}

/// Treating an IPv4-mapped IPv6 peer as inside an IPv4 network is how a
/// "trusted" range quietly widens.
#[test]
fn an_ipv4_mapped_peer_does_not_match_an_ipv4_network() {
    let net = parse_network("10.0.0.0/8").unwrap();
    assert!(!net.contains(&ip("::ffff:10.0.0.1")));
}

#[test]
fn a_zero_prefix_matches_everything_of_its_family() {
    let all = parse_network("0.0.0.0/0").unwrap();
    assert!(all.contains(&ip("203.0.113.9")));
    assert!(!all.contains(&ip("2001:db8::1")));
}

#[test]
fn groups_are_split_and_trimmed() {
    let mut identity = forwarded("10.0.0.1");
    identity.groups = Some("  a , b ,, c  ".to_owned());
    let principal = provider().principal(&identity).unwrap();
    assert_eq!(principal.roles.len(), 3, "empty entries are dropped");
    assert!(principal.has_role("A") && principal.has_role("B") && principal.has_role("C"));
}

#[test]
fn no_groups_is_no_roles() {
    let mut identity = forwarded("10.0.0.1");
    identity.groups = None;
    assert!(provider().principal(&identity).unwrap().roles.is_empty());
}
