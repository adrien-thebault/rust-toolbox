//! Trusting an authenticating reverse proxy.
//!
//! Oauth2-proxy, Authelia, Cloudflare Access, Tailscale and Authentik's
//! forward-auth all already did the authentication, and the remaining work is
//! reading a few headers. Doing that *safely* is the part worth encoding.
//!
//! # Read this before enabling it
//!
//! A spoofable header is total authentication bypass. If anything can reach
//! this service without going through the proxy - a pod IP, a port-forward, a
//! misconfigured ingress - it can set the user header to anything and be that
//! user.
//!
//! So the trusted-proxy list is **mandatory**: this refuses to construct
//! without one. There is no default and no "trust everything in development"
//! mode, because that mode is what reaches production.

use std::{collections::BTreeMap, net::IpAddr};

use async_trait::async_trait;
use ipnet::IpNet;
use tracing::warn;

use super::{Credential, IdentityProvider};
use crate::principal::{AuthError, Principal};

/// Which headers carry the forwarded identity.
///
/// Proxies disagree: oauth2-proxy sets `X-Forwarded-*`, Authelia sets
/// `Remote-*`, oauth2-proxy in its other mode sets `X-Auth-Request-*`. The
/// transport layer reads whichever three this names.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ForwardedHeaders {
    /// The header carrying the username.
    pub user: String,
    /// The header carrying the groups, comma-separated.
    pub groups: String,
    /// The header carrying the email.
    pub email: String,
}

impl Default for ForwardedHeaders {
    /// The oauth2-proxy names.
    fn default() -> Self {
        Self {
            user: "x-forwarded-user".to_owned(),
            groups: "x-forwarded-groups".to_owned(),
            email: "x-forwarded-email".to_owned(),
        }
    }
}

impl ForwardedHeaders {
    /// The Authelia names.
    #[must_use]
    pub fn authelia() -> Self {
        Self {
            user: "remote-user".to_owned(),
            groups: "remote-groups".to_owned(),
            email: "remote-email".to_owned(),
        }
    }

    /// The oauth2-proxy `X-Auth-Request-*` names.
    #[must_use]
    pub fn x_auth_request() -> Self {
        Self {
            user: "x-auth-request-user".to_owned(),
            groups: "x-auth-request-groups".to_owned(),
            email: "x-auth-request-email".to_owned(),
        }
    }
}

/// Parse `10.0.0.0/8`, `2001:db8::/32` or a bare address, which is a host
/// route.
///
/// # Arguments
///
/// * `text` - A CIDR block, or a bare address which becomes a host route.
///
/// # Errors
/// [`AuthError::Malformed`] when the text is not a network.
pub fn parse_network(text: &str) -> Result<IpNet, AuthError> {
    if let Ok(net) = text.parse::<IpNet>() {
        return Ok(net);
    }
    text.parse::<IpAddr>()
        .map(IpNet::from)
        .map_err(|_| AuthError::Malformed(format!("`{text}` is not an IP address or network")))
}

/// What a proxy told us about the caller.
#[derive(Debug, Clone, Default)]
pub struct ForwardedIdentity {
    /// The user.
    pub user: Option<String>,
    /// The groups, comma-separated.
    pub groups: Option<String>,
    /// The email.
    pub email: Option<String>,
    /// The address the request actually arrived from.
    pub peer: Option<IpAddr>,
}

/// Trusts an authenticating reverse proxy's headers.
#[derive(Debug, Clone)]
pub struct ForwardedIdentityProvider {
    /// The registry id and `Principal::issuer` for these logins.
    id: String,
    /// The button label a login page shows.
    display_name: String,
    /// Which headers to read.
    headers: ForwardedHeaders,
    /// The peers allowed to assert an identity.
    trusted: Vec<IpNet>,
    /// Whether forwarded groups are uppercased into roles.
    uppercase_roles: bool,
}

impl ForwardedIdentityProvider {
    /// Build one, trusting exactly these peers.
    ///
    /// # Arguments
    ///
    /// * `trusted_proxies` - The peers allowed to assert an identity, as CIDR
    ///   blocks or bare addresses. An empty list is refused rather than read as
    ///   `trust everyone`.
    ///
    /// # Errors
    /// [`AuthError::Malformed`] when the list is empty or an entry does not
    /// parse. An empty list is refused rather than treated as "trust nothing",
    /// because a provider that can never authenticate anyone is a
    /// misconfiguration, not a safe default.
    pub fn new(trusted_proxies: &[&str]) -> Result<Self, AuthError> {
        if trusted_proxies.is_empty() {
            return Err(AuthError::Malformed(
                "ForwardedIdentityProvider needs at least one trusted proxy; a spoofable header is total authentication bypass"
                    .to_owned(),
            ));
        }
        let trusted = trusted_proxies
            .iter()
            .map(|t| parse_network(t))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            id: "forwarded".to_owned(),
            display_name: "forwarded".to_owned(),
            headers: ForwardedHeaders::default(),
            trusted,
            uppercase_roles: true,
        })
    }

    /// Override the id.
    ///
    /// # Arguments
    ///
    /// * `id` - The registry id for this provider.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// Override the button label.
    ///
    /// # Arguments
    ///
    /// * `name` - What a login page shows on the button.
    #[must_use]
    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = name.into();
        self
    }

    /// Read a different set of headers.
    ///
    /// # Arguments
    ///
    /// * `headers` - The header names this proxy uses. Defaults to
    ///   oauth2-proxy's `X-Forwarded-*`.
    #[must_use]
    pub fn with_headers(mut self, headers: ForwardedHeaders) -> Self {
        self.headers = headers;
        self
    }

    /// Whether groups are uppercased into roles.
    ///
    /// # Arguments
    ///
    /// * `uppercase` - Whether to uppercase the forwarded groups as they become
    ///   roles.
    #[must_use]
    pub fn with_uppercase_roles(mut self, uppercase: bool) -> Self {
        self.uppercase_roles = uppercase;
        self
    }

    /// The button label.
    #[must_use]
    pub fn display_name(&self) -> &str {
        &self.display_name
    }

    /// Which headers this provider reads.
    #[must_use]
    pub fn headers(&self) -> &ForwardedHeaders {
        &self.headers
    }

    /// Whether this peer is allowed to assert an identity.
    ///
    /// # Arguments
    ///
    /// * `peer` - The TCP peer address, not anything the request claimed. That
    ///   is the whole distinction this type exists to keep.
    #[must_use]
    pub fn trusts(&self, peer: IpAddr) -> bool {
        self.trusted.iter().any(|net| net.contains(&peer))
    }

    /// Turn forwarded headers into a principal, if the peer may assert them.
    ///
    /// # Arguments
    ///
    /// * `forwarded` - The headers, plus the peer that sent them. The peer is
    ///   checked before a single header is read.
    ///
    /// # Errors
    /// [`AuthError::Unauthenticated`] when the peer is not trusted, is
    /// unknown, or asserted no user.
    pub fn principal(&self, forwarded: &ForwardedIdentity) -> Result<Principal, AuthError> {
        let peer = forwarded.peer.ok_or_else(|| {
            // No peer means no way to know whether the headers are the proxy's.
            warn!("a forwarded identity arrived with no peer address; refusing it");
            AuthError::Unauthenticated
        })?;

        if !self.trusts(peer) {
            warn!(
                %peer,
                "a request from an untrusted peer carried forwarded-identity headers"
            );
            return Err(AuthError::Unauthenticated);
        }

        let user = forwarded
            .user
            .as_deref()
            .filter(|u| !u.is_empty())
            .ok_or(AuthError::Unauthenticated)?;

        let roles = forwarded
            .groups
            .as_deref()
            .unwrap_or_default()
            .split(',')
            .map(str::trim)
            .filter(|g| !g.is_empty())
            .map(|g| {
                if self.uppercase_roles {
                    g.to_ascii_uppercase()
                } else {
                    g.to_owned()
                }
            })
            .collect();

        Ok(Principal {
            subject: user.to_owned(),
            issuer: self.id.clone(),
            roles,
            display_name: Some(user.to_owned()),
            email: forwarded.email.clone().filter(|e| !e.is_empty()),
            attributes: BTreeMap::new(),
        })
    }
}

#[async_trait]
impl IdentityProvider for ForwardedIdentityProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn authenticate(&self, credential: &Credential) -> Option<Result<Principal, AuthError>> {
        // The identity arrives on headers, not as a posted credential, so the
        // transport layer wraps the parsed headers in `Credential::Custom`.
        let Credential::Custom(any) = credential else {
            return None;
        };
        let forwarded = any.downcast_ref::<ForwardedIdentity>()?;
        Some(self.principal(forwarded))
    }
}
