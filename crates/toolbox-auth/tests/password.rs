use std::collections::BTreeMap;

use async_trait::async_trait;
use secrecy::SecretString;
use toolbox_auth::{
    AuthError, Credential, IdentityProvider, PasswordProvider, Principal, ProviderRegistry,
    StoredUser, UserStore, hash_password, verify_password,
};

struct Users(Option<StoredUser>);

#[async_trait]
impl UserStore for Users {
    async fn lookup(&self, username: &str) -> Result<Option<StoredUser>, AuthError> {
        Ok(self.0.clone().filter(|u| u.subject == username))
    }
}

fn store(password: &str) -> Users {
    Users(Some(StoredUser {
        subject: "ada".to_owned(),
        password_hash: hash_password(password).unwrap(),
        roles: vec!["ADMIN".to_owned()],
        display_name: Some("Ada".to_owned()),
        email: None,
        attributes: BTreeMap::new(),
    }))
}

fn credential(username: &str, password: &str) -> Credential {
    Credential::Password {
        username: username.to_owned(),
        password: SecretString::from(password.to_owned()),
    }
}

#[test]
fn a_hash_is_phc_format_and_salted() {
    let a = hash_password("hunter2").unwrap();
    let b = hash_password("hunter2").unwrap();
    assert!(a.starts_with("$argon2id$"), "{a}");
    assert_ne!(a, b, "the same password hashes differently every time");
    assert!(verify_password("hunter2", &a));
    assert!(!verify_password("hunter3", &a));
}

#[test]
fn a_malformed_hash_does_not_verify_anything() {
    assert!(!verify_password("hunter2", "not a phc string"));
    assert!(!verify_password("", ""));
}

#[tokio::test]
async fn the_right_password_produces_a_principal() {
    let provider = PasswordProvider::new(store("hunter2"));
    let result = provider
        .authenticate(&credential("ada", "hunter2"))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(result.subject, "ada");
    assert!(result.has_role("ADMIN"));
    assert_eq!(result.issuer, "password");
}

#[tokio::test]
async fn the_wrong_password_is_refused() {
    let provider = PasswordProvider::new(store("hunter2"));
    let err = provider
        .authenticate(&credential("ada", "wrong"))
        .await
        .unwrap()
        .unwrap_err();
    assert_eq!(err, AuthError::Unauthenticated);
}

/// Returning early for an unknown username leaks which usernames exist through
/// response time, which turns the login endpoint into a user enumerator.
#[tokio::test]
async fn an_unknown_username_is_refused_the_same_way_as_a_wrong_password() {
    let provider = PasswordProvider::new(store("hunter2"));
    let unknown = provider
        .authenticate(&credential("nobody", "hunter2"))
        .await
        .unwrap();
    let wrong = provider
        .authenticate(&credential("ada", "wrong"))
        .await
        .unwrap();
    assert_eq!(unknown.unwrap_err(), wrong.unwrap_err());
}

/// A provider hands back None for a credential that is not its kind, so the
/// registry tries the next one rather than failing the login.
#[tokio::test]
async fn a_credential_of_the_wrong_kind_is_passed_on() {
    let provider = PasswordProvider::new(store("hunter2"));
    let result = provider
        .authenticate(&Credential::ApiKey(SecretString::from("k".to_owned())))
        .await;
    assert!(result.is_none());
}

#[tokio::test]
async fn a_registry_tries_providers_in_order() {
    let registry = ProviderRegistry::new().with(PasswordProvider::new(store("hunter2")));
    assert_eq!(registry.ids(), ["password"]);

    let principal: Principal = registry
        .authenticate(&credential("ada", "hunter2"))
        .await
        .unwrap();
    assert_eq!(principal.subject, "ada");
}

#[tokio::test]
async fn a_registry_with_nothing_that_claims_the_credential_is_unauthenticated() {
    let registry = ProviderRegistry::new().with(PasswordProvider::new(store("hunter2")));
    let err = registry
        .authenticate(&Credential::ApiKey(SecretString::from("k".to_owned())))
        .await
        .unwrap_err();
    assert_eq!(err, AuthError::Unauthenticated);
}

/// Adding a provider becomes a deployment change rather than a frontend
/// release, which is the whole value of this endpoint.
#[tokio::test]
async fn the_registry_describes_itself_for_a_login_page() {
    let registry =
        ProviderRegistry::new().with(PasswordProvider::new(store("x")).display_name("Staff login"));
    let info = registry.info();
    assert_eq!(info.len(), 1);
    assert_eq!(info[0].display_name, "Staff login");
    assert_eq!(info[0].kind, toolbox_auth::ProviderKind::Credential);
}

/// A Debug of a login request ends up in a log.
#[test]
fn debug_never_prints_a_credential() {
    let rendered = format!("{:?}", credential("ada", "hunter2"));
    assert!(!rendered.contains("hunter2"), "{rendered}");
    assert!(rendered.contains("redacted"), "{rendered}");
}

/// The dummy hash has to be a *valid* PHC string, or `verify_password` returns
/// false without doing any work and the timing equalisation it exists for does
/// nothing at all.
#[test]
fn the_unknown_user_path_actually_hashes() {
    use std::time::Instant;

    let provider = PasswordProvider::new(store("hunter2"));
    let runtime = tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap();

    let time = |username: &'static str| {
        let started = Instant::now();
        runtime.block_on(provider.authenticate(&credential(username, "wrong")));
        started.elapsed()
    };

    // Warm up, so the first argon2 run's page faults do not skew the pair.
    let _ = time("ada");

    let known = time("ada");
    let unknown = time("nobody");

    // argon2 at these parameters is tens of milliseconds; a short circuit is
    // microseconds. A loose bound is enough to tell those apart and will not
    // flake on a loaded machine.
    assert!(
        unknown.as_micros() * 10 > known.as_micros(),
        "an unknown username short-circuited: known={known:?} unknown={unknown:?}"
    );
}
