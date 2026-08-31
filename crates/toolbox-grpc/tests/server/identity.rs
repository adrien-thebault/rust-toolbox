use std::{
    convert::Infallible,
    sync::{Arc, Mutex},
};

use http::{Request, Response};
use secrecy::SecretString;
use toolbox_auth::{
    ForwardedPrincipal, ForwardedPrincipalProvider, JwtIdentityProvider, Principal,
    ProviderRegistry,
};
use toolbox_grpc::{X_FWD_PRINCIPAL, identity};
use tower::{Layer, ServiceExt, service_fn};

/// A `service_fn` that records the `Principal` extension it was called with.
macro_rules! recorder {
    ($seen:expr) => {{
        let seen = Arc::clone(&$seen);
        service_fn(move |req: Request<()>| {
            let seen = Arc::clone(&seen);
            async move {
                *seen.lock().unwrap() = req.extensions().get::<Principal>().cloned();
                Ok::<_, Infallible>(Response::new(()))
            }
        })
    }};
}

#[tokio::test]
async fn a_forwarded_principal_is_resolved_into_the_extensions() {
    let registry = Arc::new(ProviderRegistry::new().with(ForwardedPrincipalProvider::new()));
    let seen: Arc<Mutex<Option<Principal>>> = Arc::new(Mutex::new(None));
    let svc = identity::identity_layer(registry)
        .extracting(identity::forwarded_principal)
        .layer(recorder!(seen));

    let principal = Principal::new("alice", "keycloak").with_role("ADMIN");
    let mut req = Request::new(());
    req.headers_mut().insert(
        X_FWD_PRINCIPAL,
        ForwardedPrincipal::from(&principal)
            .encode()
            .parse()
            .unwrap(),
    );
    svc.clone().oneshot(req).await.unwrap();
    let resolved = seen
        .lock()
        .unwrap()
        .clone()
        .expect("a principal was resolved");
    assert_eq!(resolved.subject, "alice");
    assert!(resolved.roles.contains("ADMIN"));

    // No credential: the request still proceeds, with no principal.
    *seen.lock().unwrap() = None;
    svc.oneshot(Request::new(())).await.unwrap();
    assert!(seen.lock().unwrap().is_none());
}

#[tokio::test]
async fn a_direct_bearer_is_resolved_through_the_same_registry() {
    let issuer =
        JwtIdentityProvider::hmac(&SecretString::from("a".repeat(32)), "test-issuer").unwrap();
    let token = issuer
        .issue(&Principal::new("carol", "test-issuer").with_role("USER"))
        .unwrap();
    let registry = Arc::new(ProviderRegistry::new().with(issuer));
    let seen: Arc<Mutex<Option<Principal>>> = Arc::new(Mutex::new(None));
    let svc = identity::identity_layer(registry)
        .extracting(identity::bearer)
        .layer(recorder!(seen));

    let mut req = Request::new(());
    req.headers_mut().insert(
        http::header::AUTHORIZATION,
        format!("Bearer {token}").parse().unwrap(),
    );
    svc.oneshot(req).await.unwrap();
    assert_eq!(seen.lock().unwrap().clone().unwrap().subject, "carol");
}

#[tokio::test]
async fn a_custom_extractor_resolves_its_own_credential() {
    let issuer =
        JwtIdentityProvider::hmac(&SecretString::from("a".repeat(32)), "test-issuer").unwrap();
    let token = issuer
        .issue(&Principal::new("dave", "test-issuer"))
        .unwrap();
    let registry = Arc::new(ProviderRegistry::new().with(issuer));
    let seen: Arc<Mutex<Option<Principal>>> = Arc::new(Mutex::new(None));

    // A deployment-specific header this crate knows nothing about.
    let svc = identity::identity_layer(registry)
        .extracting(|headers| {
            headers
                .get("x-hand-rolled")
                .and_then(|v| v.to_str().ok())
                .map(|t| toolbox_auth::Credential::Bearer(SecretString::from(t)))
        })
        .layer(recorder!(seen));

    let mut req = Request::new(());
    req.headers_mut()
        .insert("x-hand-rolled", token.parse().unwrap());
    svc.oneshot(req).await.unwrap();
    assert_eq!(seen.lock().unwrap().clone().unwrap().subject, "dave");
}

#[tokio::test]
async fn a_layer_with_no_sources_resolves_nothing() {
    let registry = Arc::new(ProviderRegistry::new().with(ForwardedPrincipalProvider::new()));
    let seen: Arc<Mutex<Option<Principal>>> = Arc::new(Mutex::new(None));
    let svc = identity::identity_layer(registry).layer(recorder!(seen));

    let mut req = Request::new(());
    req.headers_mut().insert(
        X_FWD_PRINCIPAL,
        ForwardedPrincipal::from(&Principal::new("eve", "keycloak"))
            .encode()
            .parse()
            .unwrap(),
    );
    svc.oneshot(req).await.unwrap();
    assert!(
        seen.lock().unwrap().is_none(),
        "a source has to be added for the header to be read"
    );
}

#[test]
fn require_and_optional_read_the_request_extension() {
    let mut req = tonic::Request::new(());
    assert!(identity::optional(&req).is_none());
    assert!(identity::require(&req).is_err());

    req.extensions_mut()
        .insert(Principal::new("bob", "keycloak"));
    assert_eq!(identity::optional(&req).unwrap().subject, "bob");
    assert_eq!(identity::require(&req).unwrap().subject, "bob");
}
