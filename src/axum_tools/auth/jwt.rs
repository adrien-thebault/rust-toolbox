//! JWT-backed session issuance/verification, kept independent of whichever
//! [`AuthBackend`](super::AuthBackend) produced the [`User`] being signed.

use super::User;
pub use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey};
use jsonwebtoken::{Header, Validation, decode, encode};
use serde::Deserialize as _;
use serde_derive::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

/// JWT claims per [RFC 7519 §4.1](https://www.rfc-editor.org/rfc/rfc7519#section-4.1).
/// Every registered claim is optional here; which ones are actually required
/// is controlled by [`JwtCodec`]'s validation settings instead. `extra`
/// holds every other claim, including `User::roles` (not a registered
/// claim) - see [`Claims::for_user`]/`impl From<Claims> for User`.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Claims {
    /// the subject the token identifies (`User::subject`)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sub: Option<String>,
    /// issued-at time, as a Unix timestamp
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iat: Option<i64>,
    /// not-before time, as a Unix timestamp
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub nbf: Option<i64>,
    /// expiration time, as a Unix timestamp
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub exp: Option<i64>,
    /// the issuer, set/checked by [`JwtCodec::issuer`]
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub iss: Option<String>,
    /// the audience, set/checked by [`JwtCodec::audience`]. Only the
    /// single-string form of `aud` is modeled - a token carrying the array
    /// form allowed by RFC 7519 fails to decode (fail-closed).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub aud: Option<String>,
    /// every other claim, private or unrecognized
    #[serde(flatten)]
    pub extra: HashMap<String, serde_json::Value>,
}

impl Claims {
    /// builds session claims for `user`, valid from now (`nbf`/`iat`) for
    /// `ttl`. `iss`/`aud` start unset - [`JwtCodec::encode`] fills them in.
    /// `user.roles` goes into `extra["roles"]`.
    pub fn for_user(user: &User, ttl: chrono::Duration) -> Self {
        let now = chrono::Utc::now();
        let mut extra = HashMap::new();
        extra.insert("roles".to_string(), serde_json::json!(user.roles));
        Self {
            sub: Some(user.subject.clone()),
            iat: Some(now.timestamp()),
            nbf: Some(now.timestamp()),
            exp: Some((now + ttl).timestamp()),
            extra,
            ..Default::default()
        }
    }
}

impl From<Claims> for User {
    fn from(claims: Claims) -> Self {
        let roles = claims
            .extra
            .get("roles")
            .and_then(|roles| Vec::<String>::deserialize(roles).ok())
            .unwrap_or_default();
        Self {
            subject: claims.sub.unwrap_or_default(),
            roles,
        }
    }
}

/// implemented by an axum app's state so the [`User`] extractor can decode a
/// session without knowing anything else about that state.
pub trait JwtCodecProvider {
    /// this state's [`JwtCodec`]
    fn jwt_codec(&self) -> &JwtCodec;
}

/// errors from [`JwtCodec::encode`]/[`JwtCodec::decode`].
#[derive(Debug, Error)]
pub enum JwtError {
    /// signing failed - a server-side problem (e.g. a malformed key), not
    /// the caller's fault
    #[error("failed to sign JWT: {0}")]
    Encode(#[source] jsonwebtoken::errors::Error),
    /// the token doesn't parse, its signature doesn't check out, or it
    /// fails one of this codec's configured validations (expired, wrong
    /// issuer/audience, ...)
    #[error("invalid or expired JWT: {0}")]
    Decode(#[source] jsonwebtoken::errors::Error),
}

/// encodes/decodes session JWTs. [`new`](Self::new) matches
/// `jsonwebtoken`'s own defaults (HS256, `exp` required) plus a mandatory
/// `sub` - override the rest with the consuming builder methods below. Not
/// limited to HMAC: pass `EncodingKey`/`DecodingKey::from_rsa_pem` plus
/// [`Self::algorithm`]`(Algorithm::RS256)` for RSA, etc.
pub struct JwtCodec {
    encoding_key: EncodingKey,
    decoding_key: DecodingKey,
    algorithm: Algorithm,
    issuer: Option<String>,
    audience: Option<String>,
    validation: Validation,
}

impl JwtCodec {
    /// `jsonwebtoken`'s own defaults (HS256, `exp` mandatory/validated, no
    /// `nbf`/`iss`/`aud` check) plus one deviation: `sub` is required too.
    /// This is a session codec - every session identifies a subject, and
    /// without the requirement a validly-signed token missing `sub` would
    /// decode into a [`User`] with an empty subject.
    pub fn new(encoding_key: EncodingKey, decoding_key: DecodingKey) -> Self {
        let mut validation = Validation::default();
        validation.required_spec_claims.insert("sub".to_string());
        Self {
            encoding_key,
            decoding_key,
            algorithm: Algorithm::default(),
            issuer: None,
            audience: None,
            validation,
        }
    }

    /// overrides the signing/verification algorithm (default: `HS256`)
    pub fn algorithm(mut self, algorithm: Algorithm) -> Self {
        self.algorithm = algorithm;
        self.validation.algorithms = vec![algorithm];
        self
    }

    /// whether to check the `nbf` claim (default: `false`, matching
    /// `jsonwebtoken`'s own default - it's an optional claim per RFC 7519)
    pub fn validate_nbf(mut self, validate: bool) -> Self {
        self.validation.validate_nbf = validate;
        self
    }

    /// sets the `iss` claim this codec stamps on every token it encodes,
    /// and then requires and checks on every token it decodes
    pub fn issuer(mut self, issuer: impl Into<String>) -> Self {
        let issuer = issuer.into();
        self.validation.set_issuer(&[&issuer]);
        // jsonwebtoken only checks `iss` against `correct_iss` when the
        // token actually carries an `iss` claim - a token missing it
        // entirely otherwise passes silently. Requiring it closes that
        // gap: configuring an issuer means every decoded token must have
        // one.
        self.validation
            .required_spec_claims
            .insert("iss".to_string());
        self.issuer = Some(issuer);
        self
    }

    /// sets the `aud` claim this codec stamps on every token it encodes,
    /// and then requires and checks on every token it decodes (same
    /// required-claim reasoning as [`Self::issuer`])
    pub fn audience(mut self, audience: impl Into<String>) -> Self {
        let audience = audience.into();
        self.validation.set_audience(&[&audience]);
        self.validation
            .required_spec_claims
            .insert("aud".to_string());
        self.audience = Some(audience);
        self
    }

    /// signs `claims` into a JWT, after stamping `iss`/`aud` from this
    /// codec's own configuration (overwriting whatever `claims` carried, so
    /// an encoded token's `iss`/`aud` always matches what this codec itself
    /// validates on decode).
    pub fn encode(&self, claims: &Claims) -> Result<String, JwtError> {
        let claims = Claims {
            iss: self.issuer.clone(),
            aud: self.audience.clone(),
            ..claims.clone()
        };
        encode(&Header::new(self.algorithm), &claims, &self.encoding_key).map_err(JwtError::Encode)
    }

    /// decodes and validates a JWT, returning its claims
    pub fn decode(&self, token: &str) -> Result<Claims, JwtError> {
        decode::<Claims>(token, &self.decoding_key, &self.validation)
            .map(|data| data.claims)
            .map_err(JwtError::Decode)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn user() -> User {
        User {
            subject: "admin".to_string(),
            roles: vec!["admin".to_string()],
        }
    }

    fn codec(secret: &str) -> JwtCodec {
        JwtCodec::new(
            EncodingKey::from_secret(secret.as_bytes()),
            DecodingKey::from_secret(secret.as_bytes()),
        )
    }

    #[test]
    fn jwt_round_trips() {
        let codec = codec("secret");
        let claims = Claims::for_user(&user(), chrono::Duration::minutes(5));
        let token = codec.encode(&claims).unwrap();
        let decoded = codec.decode(&token).expect("valid jwt");
        assert_eq!(User::from(decoded), user());
    }

    #[test]
    fn jwt_rejects_wrong_secret() {
        let claims = Claims::for_user(&user(), chrono::Duration::minutes(5));
        let token = codec("secret").encode(&claims).unwrap();
        assert!(codec("other").decode(&token).is_err());
    }

    #[test]
    fn decode_exposes_standard_claims() {
        let codec = codec("secret");
        let claims = Claims::for_user(&user(), chrono::Duration::minutes(5));
        let token = codec.encode(&claims).unwrap();
        let decoded = codec.decode(&token).expect("decodes");
        assert_eq!(decoded.sub.as_deref(), Some("admin"));
        assert_eq!(decoded.nbf, decoded.iat);
        assert!(decoded.exp > decoded.iat);
        assert_eq!(decoded.iss, None);
        assert_eq!(decoded.aud, None);
    }

    #[test]
    fn nbf_in_the_future_is_ignored_unless_opted_into() {
        let mut claims = Claims::for_user(&user(), chrono::Duration::minutes(5));
        claims.nbf = Some((chrono::Utc::now() + chrono::Duration::hours(1)).timestamp());

        let lenient = codec("secret");
        let token = lenient.encode(&claims).unwrap();
        assert!(lenient.decode(&token).is_ok());

        let strict = codec("secret").validate_nbf(true);
        assert!(strict.decode(&token).is_err());
    }

    #[test]
    fn token_without_sub_is_rejected() {
        let codec = codec("secret");
        let mut claims = Claims::for_user(&user(), chrono::Duration::minutes(5));
        claims.sub = None;
        let token = codec.encode(&claims).unwrap();
        assert!(codec.decode(&token).is_err());
    }

    #[test]
    fn issuer_and_audience_are_stamped_and_enforced() {
        let scoped = codec("secret")
            .issuer("example.web")
            .audience("example.admin");

        let claims = Claims::for_user(&user(), chrono::Duration::minutes(5));
        let token = scoped.encode(&claims).unwrap();
        let decoded = scoped.decode(&token).expect("valid jwt");
        assert_eq!(decoded.iss.as_deref(), Some("example.web"));
        assert_eq!(decoded.aud.as_deref(), Some("example.admin"));

        // a token from a codec with no configured audience isn't accepted
        // by one that requires one
        let unscoped = codec("secret");
        let token = unscoped.encode(&claims).unwrap();
        assert!(scoped.decode(&token).is_err());
    }

    #[test]
    fn unmodeled_claims_round_trip_through_extra() {
        let codec = codec("secret");
        let mut claims = Claims::for_user(&user(), chrono::Duration::minutes(5));
        claims
            .extra
            .insert("email".to_string(), serde_json::json!("user@example.com"));

        let token = codec.encode(&claims).unwrap();
        let decoded = codec.decode(&token).expect("valid jwt");
        assert_eq!(
            decoded.extra.get("email"),
            Some(&serde_json::json!("user@example.com"))
        );
    }
}
