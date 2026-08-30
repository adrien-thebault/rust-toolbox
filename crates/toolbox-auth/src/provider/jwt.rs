//! The gateway's session token, and verification of a trusted issuer's.
//!
//! [`JwtIdentityProvider`] both mints the gateway's own HS256 sessions and, as
//! an [`IdentityProvider`], verifies whatever bearer token a request carries -
//! its own, or one signed by an IdP whose JWKS or public key it was given. It
//! removes two traps `jsonwebtoken` makes easy: an `iss` check that passes when
//! the claim is absent, and a `chrono::Duration` in a signature that drags a
//! datetime crate into every caller.
//!
//! # Stateless refresh
//!
//! A refresh token is a second JWT ([`TokenUse::Refresh`]), longer-lived,
//! carrying the principal and - for the password path - an opaque `epoch`
//! fingerprint of the stored credential
//! ([`crate::provider::password::auth_epoch`]).
//! [`JwtIdentityProvider::refresh`] hands what the token carried to a `resolve`
//! closure and issues tokens for whatever it returns: re-read the user there,
//! so a demotion or a disabled account takes effect on the next refresh rather
//! than after the full [`JwtIdentityProvider::refresh_ttl`], and reject when
//! `epoch` no longer matches the stored credential. There is deliberately no
//! server-side record: no per-device logout, and no replay detection beyond
//! that check. Keep `refresh_ttl` short.

use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tracing::debug;
#[cfg(feature = "jwks")]
use tracing::warn;

use super::{Credential, IdentityProvider};
use crate::principal::{AuthError, Principal, mapping::PrincipalMapping};

/// Default access-token lifetime.
///
/// Short on purpose. A JWT cannot be revoked, so its lifetime *is* the
/// revocation window.
pub const DEFAULT_TTL: Duration = Duration::from_secs(15 * 60);

/// Default refresh-token lifetime.
///
/// How long a token survives a `resolve` closure that does not re-read the
/// user, and - re-read or not - how long a leaked one stays useful.
pub const DEFAULT_REFRESH_TTL: Duration = Duration::from_secs(7 * 24 * 60 * 60);

/// Clock-skew leeway, in seconds. Enough for ordinary drift between replicas,
/// not enough to meaningfully extend a token's life.
const DEFAULT_LEEWAY: u64 = 60;

/// Which kind of token a set of [`Claims`] describes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TokenUse {
    /// A short-lived access token, sent on every request.
    Access,
    /// A long-lived refresh token, exchanged for a new access token.
    Refresh,
}

/// The registered claims, plus the ones a principal needs.
///
/// Times are epoch seconds computed from `SystemTime`, so this crate has no
/// datetime dependency at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Claims {
    /// Subject.
    pub sub: String,
    /// Issuer: **this gateway**, which is what verification checks.
    pub iss: String,
    /// Which identity provider authenticated the subject.
    ///
    /// A separate claim from `iss` on purpose. They are different questions -
    /// "who signed this token" and "who vouched for this person" - and
    /// conflating them makes a token fail its own issuer check the moment a
    /// second provider exists.
    #[serde(default)]
    pub idp: String,
    /// Audience.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
    /// Expiry, epoch seconds.
    pub exp: u64,
    /// Issued at, epoch seconds.
    pub iat: u64,
    /// Access or refresh. An access token presented where a refresh token is
    /// required, or the reverse, is refused.
    pub token_use: TokenUse,
    /// Opaque credential fingerprint, on a refresh token bound to a password.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub epoch: Option<String>,
    /// Roles.
    #[serde(default)]
    pub roles: BTreeSet<String>,
    /// Display name.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// Email.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Anything else the provider supplied.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
}

impl Claims {
    /// The fields shared by an access token and a refresh token.
    fn base(
        principal: &Principal,
        issuer: &str,
        ttl: Duration,
        audience: Option<&str>,
        token_use: TokenUse,
    ) -> Self {
        let now = epoch_seconds(SystemTime::now());
        Self {
            sub: principal.subject.clone(),
            iss: issuer.to_owned(),
            idp: principal.issuer.clone(),
            aud: audience.map(ToOwned::to_owned),
            exp: now.saturating_add(ttl.as_secs()),
            iat: now,
            token_use,
            epoch: None,
            roles: principal.roles.clone(),
            name: principal.display_name.clone(),
            email: principal.email.clone(),
            attributes: principal.attributes.clone(),
        }
    }

    /// Access-token claims for a principal, valid for `ttl`.
    ///
    /// # Arguments
    ///
    /// * `principal` - Who the token is for. Its own issuer becomes `idp`.
    /// * `issuer` - This gateway, which is what verification checks.
    /// * `ttl` - How long the token is valid.
    /// * `audience` - Who the token is for, when a deployment has more than one
    ///   API validating the same tokens.
    #[must_use]
    pub fn for_access(
        principal: &Principal,
        issuer: &str,
        ttl: Duration,
        audience: Option<&str>,
    ) -> Self {
        Self::base(principal, issuer, ttl, audience, TokenUse::Access)
    }

    /// Refresh-token claims for a principal, valid for `ttl`.
    ///
    /// # Arguments
    ///
    /// * `principal` - Who the token is for.
    /// * `issuer` - This gateway.
    /// * `ttl` - How long the refresh token is valid.
    /// * `audience` - The audience, if one is configured.
    /// * `epoch` - The credential fingerprint to bind the token to, or `None`
    ///   for a token with no credential binding.
    #[must_use]
    pub fn for_refresh(
        principal: &Principal,
        issuer: &str,
        ttl: Duration,
        audience: Option<&str>,
        epoch: Option<String>,
    ) -> Self {
        Self {
            epoch,
            ..Self::base(principal, issuer, ttl, audience, TokenUse::Refresh)
        }
    }

    /// The principal these claims describe.
    #[must_use]
    pub fn to_principal(&self) -> Principal {
        Principal {
            subject: self.sub.clone(),
            // The identity provider, not the gateway that signed the token.
            issuer: if self.idp.is_empty() {
                self.iss.clone()
            } else {
                self.idp.clone()
            },
            roles: self.roles.clone(),
            display_name: self.name.clone(),
            email: self.email.clone(),
            attributes: self.attributes.clone(),
        }
    }
}

/// A `SystemTime` as seconds since the epoch, which is how JWT times are
/// expressed and why this crate needs no datetime dependency.
///
/// # Arguments
///
/// * `t` - The instant to convert. A time before the epoch clamps to zero.
fn epoch_seconds(t: SystemTime) -> u64 {
    t.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
}

/// A newly issued access and refresh pair, and the principal they stand for.
#[derive(Debug, Clone)]
pub struct Refreshed {
    /// Who the tokens authenticate.
    pub principal: Principal,
    /// The new access token.
    pub access_token: String,
    /// The rolled refresh token, sliding the window.
    pub refresh_token: String,
}

/// What a refresh token carried, handed to [`JwtIdentityProvider::refresh`]'s
/// `resolve` closure.
#[derive(Debug, Clone)]
pub struct RefreshInfo {
    /// The subject the token was issued for.
    pub subject: String,
    /// The identity provider that originally authenticated the subject.
    pub idp: String,
    /// The credential fingerprint the token was bound to, if any. Compare it
    /// against the user's current one to catch a changed password.
    pub epoch: Option<String>,
    /// The principal exactly as the token froze it - roles and all. Stale by
    /// definition: re-read your store rather than trusting it.
    pub stale: Principal,
}

/// How a bearer token is verified.
enum Key {
    /// Symmetric. Signs and verifies the gateway's own sessions.
    Hmac {
        /// The signing key.
        encoding: EncodingKey,
        /// The verifying key. Same secret.
        decoding: DecodingKey,
    },
    /// A single PEM public key. Verifies a third party's token; cannot sign.
    Public(DecodingKey),
    /// A fetched, rotating JWKS. Verifies a third party's token; cannot sign.
    #[cfg(feature = "jwks")]
    Jwks(JwksKeys),
}

/// Signs the gateway's sessions and verifies a bearer token, its own or a
/// trusted issuer's.
pub struct JwtIdentityProvider {
    /// The registry id.
    id: String,
    /// The key strategy.
    key: Key,
    /// The `iss` written on issue and required on verify.
    issuer: String,
    /// The `aud` set on issue and required on verify, when configured.
    audience: Option<String>,
    /// Access-token lifetime.
    ttl: Duration,
    /// Refresh-token lifetime.
    refresh_ttl: Duration,
    /// Clock-skew leeway, seconds.
    leeway: u64,
    /// How to read a principal out of an asymmetric issuer's claims. Unused for
    /// the HMAC strategy, which decodes to the typed [`Claims`].
    mapping: PrincipalMapping,
    /// Built once from the above, reused on every verification.
    validation: Validation,
}

impl std::fmt::Debug for JwtIdentityProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let strategy = match self.key {
            Key::Hmac { .. } => "hmac",
            Key::Public(_) => "public_key",
            #[cfg(feature = "jwks")]
            Key::Jwks(_) => "jwks",
        };
        f.debug_struct("JwtIdentityProvider")
            .field("id", &self.id)
            .field("strategy", &strategy)
            .field("issuer", &self.issuer)
            .field("audience", &self.audience)
            .finish_non_exhaustive()
    }
}

impl JwtIdentityProvider {
    /// An HMAC-backed provider: signs and verifies the gateway's own sessions.
    ///
    /// The secret must be at least 32 bytes; a shorter one weakens HS256 to
    /// whatever entropy it actually has.
    ///
    /// # Arguments
    ///
    /// * `secret` - The HMAC key, at least 32 bytes.
    /// * `issuer` - This gateway's name, written into `iss` and required on
    ///   verification.
    ///
    /// # Errors
    /// [`AuthError::Malformed`] when the secret is too short.
    pub fn hmac(secret: &SecretString, issuer: impl Into<String>) -> Result<Self, AuthError> {
        let bytes = secret.expose_secret().as_bytes();
        if bytes.len() < 32 {
            return Err(AuthError::Malformed(
                "the session secret must be at least 32 bytes".to_owned(),
            ));
        }
        Ok(Self::from_parts(
            Key::Hmac {
                encoding: EncodingKey::from_secret(bytes),
                decoding: DecodingKey::from_secret(bytes),
            },
            issuer.into(),
            PrincipalMapping::default(),
            vec![Algorithm::HS256],
        ))
    }

    /// A verify-only provider over one PEM public key.
    ///
    /// # Arguments
    ///
    /// * `pem` - The issuer's public key, PEM-encoded.
    /// * `algorithm` - The signature algorithm the issuer uses. Must be
    ///   asymmetric.
    /// * `issuer` - The `iss` every token must carry.
    /// * `mapping` - How to read a principal out of the issuer's claims.
    ///
    /// # Errors
    /// [`AuthError::Malformed`] when the key does not parse or the algorithm is
    /// symmetric.
    pub fn public_key(
        pem: &[u8],
        algorithm: Algorithm,
        issuer: impl Into<String>,
        mapping: PrincipalMapping,
    ) -> Result<Self, AuthError> {
        let decoding = match algorithm {
            Algorithm::RS256
            | Algorithm::RS384
            | Algorithm::RS512
            | Algorithm::PS256
            | Algorithm::PS384
            | Algorithm::PS512 => DecodingKey::from_rsa_pem(pem),
            Algorithm::ES256 | Algorithm::ES384 => DecodingKey::from_ec_pem(pem),
            Algorithm::EdDSA => DecodingKey::from_ed_pem(pem),
            _ => {
                return Err(AuthError::Malformed(
                    "public_key needs an asymmetric algorithm".to_owned(),
                ));
            }
        }
        .map_err(|e| AuthError::Malformed(format!("public key: {e}")))?;

        Ok(Self::from_parts(
            Key::Public(decoding),
            issuer.into(),
            mapping,
            vec![algorithm],
        ))
    }

    /// A verify-only provider over a third party's published JWKS.
    ///
    /// The document is fetched lazily, cached, and re-fetched when a token
    /// presents an unknown key id or the cache is stale.
    ///
    /// # Arguments
    ///
    /// * `jwks_url` - The URL of the issuer's JWKS document. Not an OIDC
    ///   discovery URL: give the `jwks_uri` directly.
    /// * `issuer` - The `iss` every token must carry.
    /// * `mapping` - How to read a principal out of the issuer's claims.
    #[cfg(feature = "jwks")]
    #[must_use]
    pub fn jwks(
        jwks_url: impl Into<String>,
        issuer: impl Into<String>,
        mapping: PrincipalMapping,
    ) -> Self {
        Self::from_parts(
            Key::Jwks(JwksKeys::new(jwks_url.into())),
            issuer.into(),
            mapping,
            vec![
                Algorithm::RS256,
                Algorithm::RS384,
                Algorithm::RS512,
                Algorithm::PS256,
                Algorithm::PS384,
                Algorithm::PS512,
                Algorithm::ES256,
                Algorithm::ES384,
                Algorithm::EdDSA,
            ],
        )
    }

    /// Assemble a provider from a key strategy and its accepted algorithms,
    /// building the reused `Validation` once.
    fn from_parts(
        key: Key,
        issuer: String,
        mapping: PrincipalMapping,
        algorithms: Vec<Algorithm>,
    ) -> Self {
        let validation = build_validation(algorithms, &issuer, None, DEFAULT_LEEWAY);
        Self {
            id: "jwt".to_owned(),
            key,
            issuer,
            audience: None,
            ttl: DEFAULT_TTL,
            refresh_ttl: DEFAULT_REFRESH_TTL,
            leeway: DEFAULT_LEEWAY,
            mapping,
            validation,
        }
    }

    /// Override the registry id.
    ///
    /// # Arguments
    ///
    /// * `id` - The id this provider is registered and looked up under.
    #[must_use]
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = id.into();
        self
    }

    /// Require this audience on every token.
    ///
    /// # Arguments
    ///
    /// * `audience` - The `aud` to set on issue and require on verify, so a
    ///   token minted for one API is rejected by another.
    #[must_use]
    pub fn audience(mut self, audience: impl Into<String>) -> Self {
        let audience = audience.into();
        self.validation.set_audience(&[&audience]);
        self.validation.validate_aud = true;
        self.validation
            .required_spec_claims
            .insert("aud".to_owned());
        self.audience = Some(audience);
        self
    }

    /// Override the clock-skew leeway.
    ///
    /// # Arguments
    ///
    /// * `seconds` - How much drift to tolerate on `exp` and `iat`.
    #[must_use]
    pub fn leeway(mut self, seconds: u64) -> Self {
        self.leeway = seconds;
        self.validation.leeway = seconds;
        self
    }

    /// Override the access-token lifetime.
    ///
    /// # Arguments
    ///
    /// * `ttl` - How long an access token lives. Short on purpose: it is the
    ///   revocation window.
    #[must_use]
    pub fn ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Override the refresh-token lifetime.
    ///
    /// # Arguments
    ///
    /// * `ttl` - How long a refresh token lives. Also how stale a principal's
    ///   roles can get, since refresh trusts what the token carried.
    #[must_use]
    pub fn refresh_ttl(mut self, ttl: Duration) -> Self {
        self.refresh_ttl = ttl;
        self
    }

    /// The configured access-token lifetime.
    #[must_use]
    pub fn token_ttl(&self) -> Duration {
        self.ttl
    }

    /// Issue an access token for a principal.
    ///
    /// # Arguments
    ///
    /// * `principal` - Who the token is for.
    ///
    /// # Errors
    /// [`AuthError::Malformed`] when this provider holds no HMAC secret, or the
    /// token cannot be encoded.
    pub fn issue(&self, principal: &Principal) -> Result<String, AuthError> {
        self.encode(&Claims::for_access(
            principal,
            &self.issuer,
            self.ttl,
            self.audience.as_deref(),
        ))
    }

    /// Issue a refresh token for a principal.
    ///
    /// # Arguments
    ///
    /// * `principal` - Who the token is for.
    /// * `epoch` - The credential fingerprint from
    ///   [`crate::provider::password::auth_epoch`], or `None` for a token with
    ///   no credential binding.
    ///
    /// # Errors
    /// [`AuthError::Malformed`] when this provider holds no HMAC secret.
    pub fn issue_refresh(
        &self,
        principal: &Principal,
        epoch: Option<&str>,
    ) -> Result<String, AuthError> {
        self.encode(&Claims::for_refresh(
            principal,
            &self.issuer,
            self.refresh_ttl,
            self.audience.as_deref(),
            epoch.map(ToOwned::to_owned),
        ))
    }

    /// Redeem a refresh token for a fresh access and refresh pair.
    ///
    /// `resolve` is handed what the token carried ([`RefreshInfo`]) and must
    /// return the principal *as it is now* - re-read from the user store, so a
    /// demotion or a disabled account takes effect here rather than after the
    /// full [`JwtIdentityProvider::refresh_ttl`]. Return
    /// `Err(AuthError::Unauthenticated)` to reject: the account is gone, or
    /// `info.epoch` no longer matches the stored credential. The new refresh
    /// token carries the same `epoch` the old one did.
    ///
    /// # Arguments
    ///
    /// * `token` - The refresh token presented.
    /// * `resolve` - Turns [`RefreshInfo`] into the current principal, or an
    ///   error to reject.
    ///
    /// # Errors
    /// [`AuthError::Expired`] for an expired token; [`AuthError::Unauthenticated`]
    /// when it is not a refresh token, the signature or issuer is wrong, or
    /// `resolve` rejected it.
    pub async fn refresh<F, Fut>(&self, token: &str, resolve: F) -> Result<Refreshed, AuthError>
    where
        F: FnOnce(RefreshInfo) -> Fut,
        Fut: std::future::Future<Output = Result<Principal, AuthError>> + Send,
    {
        let claims = self.decode(token)?;
        if claims.token_use != TokenUse::Refresh {
            return Err(AuthError::Unauthenticated);
        }
        let carried_epoch = claims.epoch.clone();
        let stale = claims.to_principal();
        let info = RefreshInfo {
            subject: claims.sub,
            idp: claims.idp,
            epoch: claims.epoch,
            stale,
        };
        let fresh = resolve(info).await?;
        Ok(Refreshed {
            access_token: self.issue(&fresh)?,
            refresh_token: self.issue_refresh(&fresh, carried_epoch.as_deref())?,
            principal: fresh,
        })
    }

    /// Verify a bearer token and recover its principal.
    ///
    /// # Arguments
    ///
    /// * `token` - The bearer token as it arrived. Signature, expiry, issuer
    ///   and audience are all checked; the HMAC strategy also refuses a refresh
    ///   token presented here.
    ///
    /// # Errors
    /// [`AuthError::Expired`] for an expired token, or
    /// [`AuthError::Unauthenticated`] for anything else.
    // The jwks strategy awaits a key fetch; the hmac and public_key ones do
    // not, so without the `jwks` feature the body has no `.await`.
    #[allow(clippy::unused_async, clippy::unused_async_trait_impl)]
    pub async fn verify(&self, token: &str) -> Result<Principal, AuthError> {
        match &self.key {
            Key::Hmac { .. } => {
                let claims = self.decode(token)?;
                if claims.token_use != TokenUse::Access {
                    debug!("a non-access token was presented for request authentication");
                    return Err(AuthError::Unauthenticated);
                }
                Ok(claims.to_principal())
            }
            Key::Public(decoding) => self.map_claims(&self.decode_value(token, decoding)?),
            #[cfg(feature = "jwks")]
            Key::Jwks(keys) => {
                let decoding = keys.decoding_for(token).await?;
                self.map_claims(&self.decode_value(token, &decoding)?)
            }
        }
    }

    /// Apply the `PrincipalMapping` to a decoded third-party token.
    fn map_claims(&self, value: &serde_json::Value) -> Result<Principal, AuthError> {
        self.mapping.apply(value, &self.issuer).ok_or_else(|| {
            AuthError::Malformed(format!(
                "the token has no `{}` claim, so there is no stable subject",
                self.mapping.subject.as_str()
            ))
        })
    }

    /// Verify and decode one of our own HS256 tokens to the typed [`Claims`].
    fn decode(&self, token: &str) -> Result<Claims, AuthError> {
        let Key::Hmac { decoding, .. } = &self.key else {
            return Err(AuthError::Malformed(
                "this verifier does not hold a symmetric key".to_owned(),
            ));
        };
        jsonwebtoken::decode::<Claims>(token, decoding, &self.validation)
            .map(|data| data.claims)
            .map_err(|e| map_jwt_err(&e))
    }

    /// Verify a third party's token and decode its claims to untyped JSON.
    fn decode_value(&self, token: &str, key: &DecodingKey) -> Result<serde_json::Value, AuthError> {
        jsonwebtoken::decode::<serde_json::Value>(token, key, &self.validation)
            .map(|data| data.claims)
            .map_err(|e| map_jwt_err(&e))
    }

    /// Sign a set of claims, or fail if this provider holds no HMAC secret.
    fn encode(&self, claims: &Claims) -> Result<String, AuthError> {
        let Key::Hmac { encoding, .. } = &self.key else {
            return Err(AuthError::Malformed(
                "a verify-only provider cannot issue tokens".to_owned(),
            ));
        };
        jsonwebtoken::encode(&Header::new(Algorithm::HS256), claims, encoding)
            .map_err(|e| AuthError::Malformed(e.to_string()))
    }
}

#[async_trait]
impl IdentityProvider for JwtIdentityProvider {
    fn id(&self) -> &str {
        &self.id
    }

    async fn authenticate(&self, credential: &Credential) -> Option<Result<Principal, AuthError>> {
        let Credential::Bearer(token) = credential else {
            return None;
        };
        Some(self.verify(token.expose_secret()).await)
    }
}

/// Turn a `jsonwebtoken` error into the coarse [`AuthError`] a caller sees.
///
/// The reason is logged, never returned: telling a caller *why* their token
/// failed is telling an attacker.
///
/// # Arguments
///
/// * `e` - The decode error.
fn map_jwt_err(e: &jsonwebtoken::errors::Error) -> AuthError {
    if matches!(e.kind(), jsonwebtoken::errors::ErrorKind::ExpiredSignature) {
        return AuthError::Expired;
    }
    debug!(error = %e, "a bearer token did not verify");
    AuthError::Unauthenticated
}

/// Build the `Validation` reused on every verification.
///
/// # Arguments
///
/// * `algorithms` - The signature algorithms this provider accepts.
/// * `issuer` - The `iss` every token must carry.
/// * `audience` - The required `aud`, if any.
/// * `leeway` - Clock-skew tolerance in seconds.
fn build_validation(
    algorithms: Vec<Algorithm>,
    issuer: &str,
    audience: Option<&str>,
    leeway: u64,
) -> Validation {
    let mut validation = Validation::new(*algorithms.first().unwrap_or(&Algorithm::HS256));
    validation.algorithms = algorithms;
    validation.leeway = leeway;
    validation.set_issuer(&[issuer]);
    // Setting `iss` on Validation checks the claim only *if present*, so a
    // token with no `iss` at all would pass. Requiring it is what makes the
    // check mean anything.
    validation.required_spec_claims.insert("iss".to_owned());
    validation.required_spec_claims.insert("exp".to_owned());
    validation.required_spec_claims.insert("sub".to_owned());
    match audience {
        Some(audience) => {
            validation.set_audience(&[audience]);
            validation.validate_aud = true;
            validation.required_spec_claims.insert("aud".to_owned());
        }
        // Without a required audience, a token carrying an unrelated `aud`
        // still verifies.
        None => validation.validate_aud = false,
    }
    validation
}

/// A fetched, rotating JWKS.
#[cfg(feature = "jwks")]
struct JwksKeys {
    /// The `jwks_uri` to fetch.
    url: String,
    /// The client used to fetch it.
    http: reqwest::Client,
    /// The last document fetched, if any.
    cache: tokio::sync::RwLock<Option<CachedJwks>>,
}

/// One cached JWKS document and when it was fetched.
#[cfg(feature = "jwks")]
struct CachedJwks {
    /// When [`JwksKeys::fetch`] last succeeded.
    fetched_at: std::time::Instant,
    /// The keys it returned.
    set: jsonwebtoken::jwk::JwkSet,
}

/// How long a fetched JWKS is trusted before a refetch.
#[cfg(feature = "jwks")]
const JWKS_TTL: Duration = Duration::from_secs(3600);

#[cfg(feature = "jwks")]
impl JwksKeys {
    /// An empty cache over a JWKS URL; nothing is fetched until first use.
    fn new(url: String) -> Self {
        Self {
            url,
            http: reqwest::Client::new(),
            cache: tokio::sync::RwLock::new(None),
        }
    }

    /// The decoding key for a token's `kid`, refetching the set on a miss.
    async fn decoding_for(&self, token: &str) -> Result<DecodingKey, AuthError> {
        let kid = jsonwebtoken::decode_header(token)
            .map_err(|e| map_jwt_err(&e))?
            .kid
            .ok_or(AuthError::Unauthenticated)?;

        if let Some(key) = self.cached(&kid).await {
            return Ok(key);
        }
        // An unknown kid or a stale cache: the signer may have rotated.
        let set = self.fetch().await?;
        let key = set
            .find(&kid)
            .ok_or(AuthError::Unauthenticated)
            .and_then(jwk_to_key)?;
        *self.cache.write().await = Some(CachedJwks {
            fetched_at: std::time::Instant::now(),
            set,
        });
        Ok(key)
    }

    /// The key for `kid` from a fresh-enough cached set, or `None`.
    async fn cached(&self, kid: &str) -> Option<DecodingKey> {
        let guard = self.cache.read().await;
        let cached = guard.as_ref()?;
        if cached.fetched_at.elapsed() >= JWKS_TTL {
            return None;
        }
        cached.set.find(kid).and_then(|jwk| jwk_to_key(jwk).ok())
    }

    /// Fetch the JWKS document over HTTP.
    async fn fetch(&self) -> Result<jsonwebtoken::jwk::JwkSet, AuthError> {
        self.http
            .get(&self.url)
            .send()
            .await
            .and_then(reqwest::Response::error_for_status)
            .map_err(|e| {
                warn!(error = %e, "a JWKS fetch failed");
                AuthError::Unauthenticated
            })?
            .json::<jsonwebtoken::jwk::JwkSet>()
            .await
            .map_err(|e| AuthError::Malformed(format!("jwks document: {e}")))
    }
}

/// Turn one JWK into a decoding key.
///
/// # Arguments
///
/// * `jwk` - The key from the fetched set.
#[cfg(feature = "jwks")]
fn jwk_to_key(jwk: &jsonwebtoken::jwk::Jwk) -> Result<DecodingKey, AuthError> {
    DecodingKey::from_jwk(jwk).map_err(|e| AuthError::Malformed(format!("jwk: {e}")))
}
