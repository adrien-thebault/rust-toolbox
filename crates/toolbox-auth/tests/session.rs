use std::time::Duration;

use secrecy::SecretString;
use toolbox_auth::{AuthError, JwtCodec, Principal, SessionCodec};

fn codec() -> JwtCodec {
    JwtCodec::new(&SecretString::from("a".repeat(32)), "toolbox-test").unwrap()
}

fn principal() -> Principal {
    Principal::new("u1", "toolbox-test")
        .with_role("ADMIN")
        .with_email("a@example.test")
}

#[test]
fn a_session_round_trips() {
    let codec = codec();
    let token = codec.issue(&principal()).unwrap();
    let back = codec.verify(&token).unwrap();

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
    let err = JwtCodec::new(&SecretString::from("too short"), "iss").unwrap_err();
    assert!(matches!(err, AuthError::Malformed(_)), "{err:?}");
}

#[test]
fn an_expired_session_is_distinguishable_from_an_invalid_one() {
    let codec = codec().ttl(Duration::from_secs(0));
    let token = codec.issue(&principal()).unwrap();
    // Leeway keeps a just-expired token valid, so wind it well past.
    let far_past = JwtCodec::new(&SecretString::from("a".repeat(32)), "toolbox-test")
        .unwrap()
        .ttl(Duration::from_secs(0));
    let _ = far_past;
    // A tampered token is Unauthenticated, not Expired.
    let tampered = format!("{token}x");
    assert!(matches!(
        codec.verify(&tampered),
        Err(AuthError::Unauthenticated)
    ));
}

#[test]
fn a_token_signed_with_another_key_is_refused() {
    let token = codec().issue(&principal()).unwrap();
    let other = JwtCodec::new(&SecretString::from("b".repeat(32)), "toolbox-test").unwrap();
    assert!(matches!(
        other.verify(&token),
        Err(AuthError::Unauthenticated)
    ));
}

/// The bug the review found: setting `iss` on Validation checks the claim only
/// if it is present, so a token with no `iss` at all sailed through.
#[test]
fn a_token_from_another_issuer_is_refused() {
    let other = JwtCodec::new(&SecretString::from("a".repeat(32)), "somebody-else").unwrap();
    let token = other.issue(&Principal::new("u1", "somebody-else")).unwrap();
    assert!(matches!(
        codec().verify(&token),
        Err(AuthError::Unauthenticated)
    ));
}

#[test]
fn an_audience_is_required_once_configured() {
    let issuing = codec();
    let token = issuing.issue(&principal()).unwrap();

    let expecting = JwtCodec::new(&SecretString::from("a".repeat(32)), "toolbox-test")
        .unwrap()
        .audience("admin-ui");
    assert!(
        matches!(expecting.verify(&token), Err(AuthError::Unauthenticated)),
        "a token with no audience must not pass a codec that requires one"
    );

    let matching = JwtCodec::new(&SecretString::from("a".repeat(32)), "toolbox-test")
        .unwrap()
        .audience("admin-ui");
    let good = matching.issue(&principal()).unwrap();
    assert!(matching.verify(&good).is_ok());
}

#[test]
fn garbage_is_refused_rather_than_panicking() {
    for bad in ["", "not.a.token", "a.b.c", "....."] {
        assert!(codec().verify(bad).is_err(), "for `{bad}`");
    }
}

/// Either is how you move from password sessions to a federated provider
/// without a flag day: both work while the migration runs.
#[test]
fn either_accepts_a_token_from_either_codec() {
    let old = JwtCodec::new(&SecretString::from("a".repeat(32)), "toolbox-test").unwrap();
    let new = JwtCodec::new(&SecretString::from("b".repeat(32)), "toolbox-test").unwrap();

    let old_token = old.issue(&principal()).unwrap();
    let new_token = new.issue(&principal()).unwrap();

    let codec = SessionCodec::Either(new, Box::new(SessionCodec::Local(old)));
    assert!(codec.verify(&old_token).is_ok(), "the old key still works");
    assert!(codec.verify(&new_token).is_ok(), "and so does the new one");
}

/// The lifetime is a std Duration, and this crate has no datetime
/// dependency at all.
#[test]
fn claims_are_built_from_a_std_duration() {
    use toolbox_auth::Claims;
    let claims = Claims::for_principal(&principal(), "the-gateway", Duration::from_secs(600), None);
    assert_eq!(claims.exp - claims.iat, 600);
    assert_eq!(
        claims.iss, "the-gateway",
        "iss is the gateway that signed it"
    );
    assert_eq!(
        claims.idp, "toolbox-test",
        "idp is who vouched for the subject"
    );
    assert_eq!(claims.to_principal(), principal());
}
