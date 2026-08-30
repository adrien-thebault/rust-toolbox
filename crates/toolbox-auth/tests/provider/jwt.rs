use std::time::Duration;

use jsonwebtoken::{Algorithm, EncodingKey, Header};
use secrecy::SecretString;
use serde_json::json;
use toolbox_auth::{
    AuthError, Claims, JwtIdentityProvider, Principal, PrincipalMapping, RefreshInfo, TokenUse,
};

fn codec() -> JwtIdentityProvider {
    JwtIdentityProvider::hmac(&SecretString::from("a".repeat(32)), "toolbox-test").unwrap()
}

/// A `resolve` that trusts the token as-is, for tests not about re-reading.
async fn keep(info: RefreshInfo) -> Result<Principal, AuthError> {
    Ok(info.stale)
}

fn principal() -> Principal {
    Principal::new("u1", "toolbox-test")
        .with_role("ADMIN")
        .with_email("a@example.test")
}

#[tokio::test]
async fn a_session_round_trips() {
    let codec = codec();
    let token = codec.issue(&principal()).unwrap();
    let back = codec.verify(&token).await.unwrap();

    assert_eq!(back.subject, "u1");
    assert!(back.has_role("ADMIN"));
    assert_eq!(back.email.as_deref(), Some("a@example.test"));
}

/// A JWT cannot be revoked, so its lifetime is the revocation window. The
/// obvious implementation used twelve hours.
#[test]
fn the_default_lifetime_is_short() {
    assert_eq!(codec().token_ttl(), Duration::from_secs(15 * 60));
}

#[test]
fn a_short_secret_is_refused_at_construction() {
    let err = JwtIdentityProvider::hmac(&SecretString::from("too short"), "iss").unwrap_err();
    assert!(matches!(err, AuthError::Malformed(_)), "{err:?}");
}

#[tokio::test]
async fn an_expired_session_is_distinguishable_from_an_invalid_one() {
    let codec = codec().ttl(Duration::from_secs(0));
    let token = codec.issue(&principal()).unwrap();
    // A tampered token is Unauthenticated, not Expired.
    let tampered = format!("{token}x");
    assert!(matches!(
        codec.verify(&tampered).await,
        Err(AuthError::Unauthenticated)
    ));
}

#[tokio::test]
async fn a_token_signed_with_another_key_is_refused() {
    let token = codec().issue(&principal()).unwrap();
    let other =
        JwtIdentityProvider::hmac(&SecretString::from("b".repeat(32)), "toolbox-test").unwrap();
    assert!(matches!(
        other.verify(&token).await,
        Err(AuthError::Unauthenticated)
    ));
}

/// Setting `iss` on Validation checks the claim only if it is present, so a
/// token with no `iss` at all would sail through without requiring it.
#[tokio::test]
async fn a_token_from_another_issuer_is_refused() {
    let other =
        JwtIdentityProvider::hmac(&SecretString::from("a".repeat(32)), "somebody-else").unwrap();
    let token = other.issue(&Principal::new("u1", "somebody-else")).unwrap();
    assert!(matches!(
        codec().verify(&token).await,
        Err(AuthError::Unauthenticated)
    ));
}

#[tokio::test]
async fn an_audience_is_required_once_configured() {
    let issuing = codec();
    let token = issuing.issue(&principal()).unwrap();

    let expecting = JwtIdentityProvider::hmac(&SecretString::from("a".repeat(32)), "toolbox-test")
        .unwrap()
        .audience("admin-ui");
    assert!(
        matches!(
            expecting.verify(&token).await,
            Err(AuthError::Unauthenticated)
        ),
        "a token with no audience must not pass a codec that requires one"
    );

    let good = expecting.issue(&principal()).unwrap();
    assert!(expecting.verify(&good).await.is_ok());
}

#[tokio::test]
async fn garbage_is_refused_rather_than_panicking() {
    for bad in ["", "not.a.token", "a.b.c", "....."] {
        assert!(codec().verify(bad).await.is_err(), "for `{bad}`");
    }
}

/// The lifetime is a std Duration, and this crate has no datetime dependency.
#[test]
fn claims_are_built_from_a_std_duration() {
    let claims = Claims::for_access(&principal(), "the-gateway", Duration::from_secs(600), None);
    assert_eq!(claims.exp - claims.iat, 600);
    assert_eq!(
        claims.iss, "the-gateway",
        "iss is the gateway that signed it"
    );
    assert_eq!(
        claims.idp, "toolbox-test",
        "idp is who vouched for the subject"
    );
    assert_eq!(claims.token_use, TokenUse::Access);
    assert_eq!(claims.to_principal(), principal());
}

// --- stateless refresh --------------------------------------------------------

#[tokio::test]
async fn a_refresh_token_round_trips_and_rolls() {
    let codec = codec();
    let refresh = codec.issue_refresh(&principal(), None).unwrap();

    let rolled = codec.refresh(&refresh, keep).await.unwrap();
    assert_eq!(rolled.principal, principal());
    assert!(codec.verify(&rolled.access_token).await.is_ok());
    assert!(
        codec.refresh(&rolled.refresh_token, keep).await.is_ok(),
        "the rolled refresh token is itself redeemable"
    );
}

/// The whole point of the `resolve` hook: roles come from the store, not from
/// whatever the refresh token froze weeks ago.
#[tokio::test]
async fn refresh_re_reads_the_principal_so_roles_are_current() {
    let codec = codec();
    // Issued as ADMIN.
    let refresh = codec.issue_refresh(&principal(), None).unwrap();

    // The store now says the user is only a VIEWER.
    let rolled = codec
        .refresh(&refresh, |info| async move {
            Ok(Principal::new(info.subject, info.idp).with_role("VIEWER"))
        })
        .await
        .unwrap();
    assert!(rolled.principal.has_role("VIEWER"));
    assert!(
        !rolled.principal.has_role("ADMIN"),
        "the frozen role is gone"
    );

    let back = codec.verify(&rolled.access_token).await.unwrap();
    assert!(back.has_role("VIEWER") && !back.has_role("ADMIN"));
}

#[tokio::test]
async fn resolve_can_reject_a_refresh() {
    let codec = codec();
    let refresh = codec.issue_refresh(&principal(), None).unwrap();
    let err = codec
        .refresh(&refresh, |_| async { Err(AuthError::Unauthenticated) })
        .await
        .unwrap_err();
    assert_eq!(err, AuthError::Unauthenticated);
}

/// A `resolve` that compares `info.epoch` against the current fingerprint is
/// how "change your password" revokes a bound refresh token.
#[tokio::test]
async fn a_resolve_that_checks_the_epoch_rejects_a_changed_credential() {
    let codec = codec();
    let refresh = codec.issue_refresh(&principal(), Some("epoch-1")).unwrap();

    let against = |current: &'static str| {
        move |info: RefreshInfo| async move {
            if info.epoch.as_deref() == Some(current) {
                Ok(info.stale)
            } else {
                Err(AuthError::Unauthenticated)
            }
        }
    };
    assert!(codec.refresh(&refresh, against("epoch-1")).await.is_ok());
    assert!(codec.refresh(&refresh, against("epoch-2")).await.is_err());
}

#[tokio::test]
async fn an_access_token_is_not_a_refresh_token_and_the_reverse() {
    let codec = codec();
    let access = codec.issue(&principal()).unwrap();
    let refresh = codec.issue_refresh(&principal(), None).unwrap();

    assert!(
        matches!(
            codec.refresh(&access, keep).await,
            Err(AuthError::Unauthenticated)
        ),
        "an access token cannot be redeemed as a refresh token"
    );
    // A refresh token presented for request auth is refused by verify().
    assert!(matches!(
        codec.verify(&refresh).await,
        Err(AuthError::Unauthenticated)
    ));
}

#[tokio::test]
async fn an_expired_refresh_token_is_expired_not_invalid() {
    // Backdate `exp` well past the leeway; there is no "issue in the past" API.
    let mut claims = Claims::for_refresh(
        &principal(),
        "toolbox-test",
        Duration::from_secs(0),
        None,
        None,
    );
    claims.exp = claims.iat.saturating_sub(3600);
    let token = jsonwebtoken::encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(&"a".repeat(32).into_bytes()),
    )
    .unwrap();

    assert!(matches!(
        codec().refresh(&token, keep).await,
        Err(AuthError::Expired)
    ));
}

// --- verifying a third party's token ----------------------------------------

const ED_PRIVATE_PEM: &[u8] = b"-----BEGIN PRIVATE KEY-----
MC4CAQAwBQYDK2VwBCIEIN3eXdbbOoLMbbVXp0qmzSMUbhnAgk9/44Eax53APivu
-----END PRIVATE KEY-----
";
const ED_PUBLIC_PEM: &[u8] = b"-----BEGIN PUBLIC KEY-----
MCowBQYDK2VwAyEA7ewehQvjka7FmeWijMPyuv+caqrHbG1sG7mmNbQEdBE=
-----END PUBLIC KEY-----
";

#[tokio::test]
async fn a_public_key_verifier_maps_an_external_token_to_a_principal() {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    let external = json!({
        "sub": "ext-1",
        "iss": "https://idp.example",
        "exp": now + 3600,
        "iat": now,
        "preferred_username": "ada",
        "groups": ["admins"],
    });
    let mut header = Header::new(Algorithm::EdDSA);
    header.kid = Some("k1".to_owned());
    let signed = jsonwebtoken::encode(
        &header,
        &external,
        &EncodingKey::from_ed_pem(ED_PRIVATE_PEM).unwrap(),
    )
    .unwrap();

    let verifier = JwtIdentityProvider::public_key(
        ED_PUBLIC_PEM,
        Algorithm::EdDSA,
        "https://idp.example",
        PrincipalMapping::authentik(),
    )
    .unwrap();

    let principal = verifier.verify(&signed).await.unwrap();
    assert_eq!(principal.subject, "ext-1");
    assert_eq!(principal.issuer, "https://idp.example");
    assert!(principal.has_role("ADMINS"));
    assert_eq!(principal.display_name.as_deref(), Some("ada"));
}

#[test]
fn a_public_key_verifier_cannot_issue() {
    let verifier = JwtIdentityProvider::public_key(
        ED_PUBLIC_PEM,
        Algorithm::EdDSA,
        "https://idp.example",
        PrincipalMapping::default(),
    )
    .unwrap();
    assert!(matches!(
        verifier.issue(&principal()),
        Err(AuthError::Malformed(_))
    ));
}

#[test]
fn a_symmetric_algorithm_is_refused_for_a_public_key() {
    assert!(
        JwtIdentityProvider::public_key(
            ED_PUBLIC_PEM,
            Algorithm::HS256,
            "iss",
            PrincipalMapping::default(),
        )
        .is_err()
    );
}
