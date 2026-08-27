//! Issuing and verifying the gateway's own session.
//!
//! It removes two traps `jsonwebtoken` makes easy - an `iss` check that
//! silently passes when the claim is absent, and a `chrono::Duration` in the
//! signature that drags a datetime crate into every caller.
//!
//! Sessions are JWTs. A JWT cannot be revoked, so its lifetime *is* the
//! revocation window - which is why the default is fifteen minutes and why
//! revoking a session means deleting its refresh token.

use std::{
    collections::{BTreeMap, BTreeSet},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation};
use secrecy::{ExposeSecret, SecretString};
use serde::{Deserialize, Serialize};
use tracing::debug;

use crate::principal::{AuthError, Principal};

/// How long an access token lives by default.
///
/// Short on purpose. A JWT cannot be revoked, so its lifetime *is* the
/// revocation window; a naive code used twelve hours.
pub const DEFAULT_TTL: Duration = Duration::from_secs(15 * 60);

/// The registered claims, plus the ones a principal needs.
///
/// Times are epoch seconds computed from `SystemTime`, so this
/// crate has no datetime dependency at all.
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
    /// Claims for a principal, valid for `ttl`.
    ///
    /// `issuer` is this gateway; the principal's own issuer becomes `idp`.
    ///
    /// `ttl` is a [`std::time::Duration`], not a `chrono::Duration`: no
    /// datetime crate appears in a toolbox signature.
    ///
    /// # Arguments
    ///
    /// * `principal` - Who the token is for. Its own issuer becomes the `idp`
    ///   claim.
    /// * `issuer` - This gateway, which is what verification checks. Not the
    ///   identity provider.
    /// * `ttl` - How long the token is valid. A JWT cannot be revoked, so this
    ///   is the revocation window.
    /// * `audience` - Who the token is for, when a deployment has more than one
    ///   API validating the same tokens.
    #[must_use]
    pub fn for_principal(
        principal: &Principal,
        issuer: &str,
        ttl: Duration,
        audience: Option<&str>,
    ) -> Self {
        let now = epoch_seconds(SystemTime::now());
        Self {
            sub: principal.subject.clone(),
            iss: issuer.to_owned(),
            idp: principal.issuer.clone(),
            aud: audience.map(ToOwned::to_owned),
            exp: now.saturating_add(ttl.as_secs()),
            iat: now,
            roles: principal.roles.clone(),
            name: principal.display_name.clone(),
            email: principal.email.clone(),
            attributes: principal.attributes.clone(),
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

/// Signs and verifies the gateway's own sessions.
pub struct JwtCodec {
    encoding: EncodingKey,
    decoding: DecodingKey,
    issuer: String,
    audience: Option<String>,
    ttl: Duration,
    leeway: u64,
}

impl std::fmt::Debug for JwtCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JwtCodec")
            .field("issuer", &self.issuer)
            .field("audience", &self.audience)
            .field("ttl", &self.ttl)
            .finish_non_exhaustive()
    }
}

impl JwtCodec {
    /// A codec over an HMAC secret.
    ///
    /// The secret must be at least 32 bytes; a shorter one weakens HS256 to
    /// whatever entropy it actually has.
    ///
    /// # Arguments
    ///
    /// * `secret` - The HMAC key, at least 32 bytes. A shorter one weakens
    ///   HS256 to whatever entropy it actually has.
    /// * `issuer` - This gateway's name, written into `iss` and required on
    ///   verification.
    ///
    /// # Errors
    /// [`AuthError::Malformed`] when the secret is too short.
    pub fn new(secret: &SecretString, issuer: impl Into<String>) -> Result<Self, AuthError> {
        let bytes = secret.expose_secret().as_bytes();
        if bytes.len() < 32 {
            return Err(AuthError::Malformed(
                "the session secret must be at least 32 bytes".to_owned(),
            ));
        }
        Ok(Self {
            encoding: EncodingKey::from_secret(bytes),
            decoding: DecodingKey::from_secret(bytes),
            issuer: issuer.into(),
            audience: None,
            ttl: DEFAULT_TTL,
            // Enough for ordinary clock drift between replicas, not enough to
            // meaningfully extend a token's life.
            leeway: 60,
        })
    }

    /// Require this audience on every token.
    ///
    /// # Arguments
    ///
    /// * `audience` - The `aud` to set on issue and require on verify, so a
    ///   token minted for one API is rejected by another.
    #[must_use]
    pub fn audience(mut self, audience: impl Into<String>) -> Self {
        self.audience = Some(audience.into());
        self
    }

    /// How long an issued token lives.
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

    /// The configured token lifetime.
    #[must_use]
    pub fn token_ttl(&self) -> Duration {
        self.ttl
    }

    /// Issue a session for a principal.
    ///
    /// # Arguments
    ///
    /// * `principal` - Who the session is for.
    ///
    /// # Errors
    /// [`AuthError::Malformed`] when the token cannot be signed.
    pub fn issue(&self, principal: &Principal) -> Result<String, AuthError> {
        let claims =
            Claims::for_principal(principal, &self.issuer, self.ttl, self.audience.as_deref());
        jsonwebtoken::encode(&Header::new(Algorithm::HS256), &claims, &self.encoding)
            .map_err(|e| AuthError::Malformed(e.to_string()))
    }

    /// Verify a session and recover its principal.
    ///
    /// # Arguments
    ///
    /// * `token` - The bearer token as it arrived. Signature, expiry, issuer
    ///   and audience are all checked.
    ///
    /// # Errors
    /// [`AuthError::Expired`] for an expired token, or
    /// [`AuthError::Unauthenticated`] for anything else - a bad signature, a
    /// wrong issuer, a wrong audience, a missing required claim.
    pub fn verify(&self, token: &str) -> Result<Principal, AuthError> {
        let mut validation = Validation::new(Algorithm::HS256);
        validation.leeway = self.leeway;

        // The bug the review found: setting `iss` on Validation checks the
        // claim *if present*, so a token with no `iss` at all passed. Requiring
        // it is what makes the check mean anything.
        validation.set_issuer(&[&self.issuer]);
        validation.required_spec_claims.insert("iss".to_owned());
        validation.required_spec_claims.insert("exp".to_owned());
        validation.required_spec_claims.insert("sub".to_owned());

        if let Some(audience) = &self.audience {
            validation.set_audience(&[audience]);
            validation.required_spec_claims.insert("aud".to_owned());
        }

        let data =
            jsonwebtoken::decode::<Claims>(token, &self.decoding, &validation).map_err(|e| {
                if matches!(e.kind(), jsonwebtoken::errors::ErrorKind::ExpiredSignature) {
                    return AuthError::Expired;
                }
                // The reason is logged, never returned: telling a caller *why*
                // their token failed is telling an attacker.
                debug!(error = %e, "session verification failed");
                AuthError::Unauthenticated
            })?;

        Ok(data.claims.to_principal())
    }
}

/// How a session is verified.
///
/// `Either` is not decoration: it is how you move from password sessions to a
/// federated identity provider without a flag day, because both work at once
/// while the migration runs.
pub enum SessionCodec {
    /// Sessions this gateway issued.
    Local(JwtCodec),
    /// Both, tried in order. Local first, because it is the cheaper check.
    Either(JwtCodec, Box<SessionCodec>),
}

impl std::fmt::Debug for SessionCodec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Local(_) => f.write_str("SessionCodec::Local"),
            Self::Either(..) => f.write_str("SessionCodec::Either"),
        }
    }
}

impl SessionCodec {
    /// Verify a token by whichever means this codec allows.
    ///
    /// # Arguments
    ///
    /// * `token` - The token to verify. `Either` tries both codecs, which is
    ///   what makes a migration between them possible without a flag day.
    ///
    /// # Errors
    /// [`AuthError`] from the last codec tried.
    pub fn verify(&self, token: &str) -> Result<Principal, AuthError> {
        match self {
            Self::Local(codec) => codec.verify(token),
            Self::Either(codec, fallback) => {
                codec.verify(token).or_else(|_| fallback.verify(token))
            }
        }
    }

    /// Issue a session, when this codec can.
    ///
    /// # Arguments
    ///
    /// * `principal` - Who the session is for. A verify-only codec fails here
    ///   rather than at startup, because a gateway may legitimately only
    ///   verify.
    ///
    /// # Errors
    /// [`AuthError::Malformed`] when this codec only verifies.
    pub fn issue(&self, principal: &Principal) -> Result<String, AuthError> {
        match self {
            Self::Local(codec) | Self::Either(codec, _) => codec.issue(principal),
        }
    }
}
