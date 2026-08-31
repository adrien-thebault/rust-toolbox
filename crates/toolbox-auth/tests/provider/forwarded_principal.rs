use secrecy::SecretString;
use toolbox_auth::{
    AuthError, Credential, ForwardedPrincipal, ForwardedPrincipalProvider, IdentityProvider,
    Principal,
};

fn principal() -> Principal {
    Principal::new("user-123", "keycloak").with_roles(["ADMIN", "STAFF"])
}

#[tokio::test]
async fn a_forwarded_principal_round_trips_through_the_registry() {
    let original = principal();
    let encoded = ForwardedPrincipal::from(&original).encode();
    let decoded = ForwardedPrincipal::decode(&encoded).unwrap();

    let resolved = ForwardedPrincipalProvider::new()
        .authenticate(&Credential::Custom(Box::new(decoded)))
        .await
        .expect("the provider claims a ForwardedPrincipal")
        .expect("and resolves it");

    assert_eq!(resolved, original);
    assert_eq!(
        resolved.issuer, "keycloak",
        "the gateway's issuer is preserved, not restamped"
    );
}

#[tokio::test]
async fn it_ignores_a_credential_that_is_not_a_forwarded_principal() {
    let out = ForwardedPrincipalProvider::new()
        .authenticate(&Credential::Bearer(SecretString::from("a-token")))
        .await;
    assert!(out.is_none(), "not this provider's credential");
}

#[tokio::test]
async fn a_forwarded_principal_with_no_subject_is_refused() {
    let empty = ForwardedPrincipal(Principal::new("", "keycloak"));
    let out = ForwardedPrincipalProvider::new()
        .authenticate(&Credential::Custom(Box::new(empty)))
        .await
        .expect("claimed");
    assert_eq!(out.unwrap_err(), AuthError::Unauthenticated);
}

#[test]
fn decode_rejects_junk() {
    assert!(matches!(
        ForwardedPrincipal::decode("not base64 !!!"),
        Err(AuthError::Malformed(_))
    ));
    // Valid base64 ("hello"), but not a principal.
    assert!(matches!(
        ForwardedPrincipal::decode("aGVsbG8="),
        Err(AuthError::Malformed(_))
    ));
}
