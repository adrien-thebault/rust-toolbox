use secrecy::SecretString;
use toolbox_auth::{
    AssertedPrincipal, AssertedPrincipalProvider, AuthError, Credential, IdentityProvider,
    Principal,
};

fn principal() -> Principal {
    Principal::new("user-123", "keycloak").with_roles(["ADMIN", "STAFF"])
}

#[tokio::test]
async fn an_asserted_principal_round_trips_through_the_registry() {
    let original = principal();
    let encoded = AssertedPrincipal::from(&original).encode();
    let decoded = AssertedPrincipal::decode(&encoded).unwrap();

    let resolved = AssertedPrincipalProvider::new()
        .authenticate(&Credential::Custom(Box::new(decoded)))
        .await
        .expect("the provider claims an AssertedPrincipal")
        .expect("and resolves it");

    assert_eq!(resolved, original);
    assert_eq!(
        resolved.issuer, "keycloak",
        "the gateway's issuer is preserved, not restamped"
    );
}

#[tokio::test]
async fn it_ignores_a_credential_that_is_not_an_asserted_principal() {
    let out = AssertedPrincipalProvider::new()
        .authenticate(&Credential::Bearer(SecretString::from("a-token")))
        .await;
    assert!(out.is_none(), "not this provider's credential");
}

#[tokio::test]
async fn an_asserted_principal_with_no_subject_is_refused() {
    let empty = AssertedPrincipal(Principal::new("", "keycloak"));
    let out = AssertedPrincipalProvider::new()
        .authenticate(&Credential::Custom(Box::new(empty)))
        .await
        .expect("claimed");
    assert_eq!(out.unwrap_err(), AuthError::Unauthenticated);
}

#[test]
fn decode_rejects_junk() {
    assert!(matches!(
        AssertedPrincipal::decode("not base64 !!!"),
        Err(AuthError::Malformed(_))
    ));
    // Valid base64 ("hello"), but not a principal.
    assert!(matches!(
        AssertedPrincipal::decode("aGVsbG8="),
        Err(AuthError::Malformed(_))
    ));
}
