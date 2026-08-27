//! Trusting an authenticating reverse proxy.
//!
//! Oauth2-proxy, Authelia, Cloudflare Access, Tailscale and Authentik's
//! forward-auth all already did the authentication, and the remaining work is
//! reading two headers. Doing that *safely* is the part worth encoding.
//!
//! # Read this before enabling it
//!
//! A spoofable header is total authentication bypass. If anything can reach
//! this service without going through the proxy - a pod IP, a port-forward, a
//! misconfigured ingress - it can set `X-Forwarded-User` to anything and be
//! that user.
//!
//! So the trusted-proxy list is **mandatory**: this refuses to construct
//! without one. There is no default and no "trust everything in development"
//! mode, because that mode is what reaches production.

use std::{collections::BTreeMap, net::IpAddr};

use async_trait::async_trait;
use ipnet::IpNet;
use tracing::warn;

use super::{Credential, IdentityProvider, ProviderInfo, ProviderKind};
use crate::principal::{AuthError, Principal};

/// The headers this reads, which are what oauth2-proxy and Authelia set.
pub const USER_HEADER: &str = "x-forwarded-user";
/// The groups header.
pub const GROUPS_HEADER: &str = "x-forwarded-groups";
/// The email header.
pub const EMAIL_HEADER: &str = "x-forwarded-email";

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
    /// The user, from `X-Forwarded-User`.
    pub user: Option<String>,
    /// The groups, from `X-Forwarded-Groups`, comma-separated.
    pub groups: Option<String>,
    /// The email, from `X-Forwarded-Email`.
    pub email: Option<String>,
    /// The address the request actually arrived from.
    pub peer: Option<IpAddr>,
}

/// Trusts an authenticating reverse proxy's headers.
#[derive(Debug, Clone)]
pub struct ProxyHeaderProvider {
    id: String,
    display_name: String,
    trusted: Vec<IpNet>,
    uppercase_roles: bool,
}

impl ProxyHeaderProvider {
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
                "ProxyHeaderProvider needs at least one trusted proxy; a spoofable header \
                 is total authentication bypass"
                    .to_owned(),
            ));
        }
        let trusted = trusted_proxies
            .iter()
            .map(|t| parse_network(t))
            .collect::<Result<Vec<_>, _>>()?;

        Ok(Self {
            id: "proxy-header".to_owned(),
            display_name: "Single sign-on".to_owned(),
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
    pub fn id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// Override the button label.
    ///
    /// # Arguments
    ///
    /// * `name` - What a login page shows on the button.
    #[must_use]
    pub fn display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = name.into();
        self
    }

    /// Whether groups are uppercased into roles.
    ///
    /// # Arguments
    ///
    /// * `uppercase` - Whether to uppercase the forwarded groups as they become
    ///   roles.
    #[must_use]
    pub fn uppercase_roles(mut self, uppercase: bool) -> Self {
        self.uppercase_roles = uppercase;
        self
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
impl IdentityProvider for ProxyHeaderProvider {
    fn id(&self) -> &str {
        &self.id
    }

    fn info(&self) -> ProviderInfo {
        ProviderInfo {
            id: self.id.clone(),
            display_name: self.display_name.clone(),
            // The proxy has already authenticated by the time a request
            // arrives, so there is nothing for a login page to start.
            kind: ProviderKind::Credential,
        }
    }

    async fn authenticate(&self, credential: &Credential) -> Option<Result<Principal, AuthError>> {
        // The identity arrives on headers, not as a posted credential, so the
        // extractor calls `principal` directly.
        let Credential::Custom(any) = credential else {
            return None;
        };
        let forwarded = any.downcast_ref::<ForwardedIdentity>()?;
        Some(self.principal(forwarded))
    }
}
