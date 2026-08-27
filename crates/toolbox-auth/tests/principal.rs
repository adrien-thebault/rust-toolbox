use toolbox_auth::{AuthError, Principal, Role};
use toolbox_core::{ErrorKind, ServiceError};

struct Admin;
impl Role for Admin {
    const NAME: &'static str = "ADMIN";
}

#[test]
fn a_principal_is_keyed_by_issuer_and_subject_together() {
    // A subject is unique only within its issuer, so two providers can each
    // have a user `1`. Anything storing a principal must key on both.
    assert_ne!(
        Principal::new("1", "keycloak").key(),
        Principal::new("1", "google").key()
    );
    assert_eq!(Principal::new("1", "keycloak").key(), "keycloak|1");
}

#[test]
fn roles_are_strings_so_the_toolbox_never_learns_what_they_mean() {
    let p = Principal::new("u", "local").with_roles(["ADMIN", "EDITOR"]);
    assert!(p.has_role("ADMIN"));
    assert!(p.has::<Admin>());
    assert!(!p.has_role("OWNER"));
}

#[test]
fn the_default_role_matches_any_authenticated_caller() {
    use toolbox_auth::principal::AnyRole;
    let p = Principal::new("u", "local");
    assert!(
        p.has::<AnyRole>(),
        "no roles at all still satisfies AnyRole"
    );
}

#[test]
fn require_role_names_the_role_that_was_needed() {
    let err = Principal::new("u", "local")
        .require_role("ADMIN")
        .unwrap_err();
    assert_eq!(
        err,
        AuthError::Forbidden {
            required: "ADMIN".to_owned()
        }
    );
    assert_eq!(err.metadata()["required_role"], "ADMIN");
}

/// An expired session is 401 so the client knows to refresh; 403 would tell it
/// to give up.
#[test]
fn an_expired_session_is_unauthenticated_not_forbidden() {
    assert_eq!(AuthError::Expired.kind(), ErrorKind::Unauthenticated);
    assert_eq!(
        AuthError::Unauthenticated.kind(),
        ErrorKind::Unauthenticated
    );
    assert_eq!(
        AuthError::Forbidden {
            required: "X".to_owned()
        }
        .kind(),
        ErrorKind::PermissionDenied
    );
}

#[test]
fn a_principal_round_trips_through_json() {
    let p = Principal::new("u1", "keycloak")
        .with_role("ADMIN")
        .with_display_name("Ada")
        .with_email("ada@example.test")
        .with_attribute("tenant", "acme");
    let back: Principal = serde_json::from_str(&serde_json::to_string(&p).unwrap()).unwrap();
    assert_eq!(p, back);
}

#[test]
fn absent_optional_fields_are_omitted_rather_than_null() {
    let json = serde_json::to_string(&Principal::new("u", "local")).unwrap();
    assert!(!json.contains("display_name"), "{json}");
    assert!(!json.contains("email"), "{json}");
    assert!(!json.contains("attributes"), "{json}");
}
