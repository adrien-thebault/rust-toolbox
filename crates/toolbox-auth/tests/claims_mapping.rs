use toolbox_auth::ClaimsMapping;

fn keycloak_token() -> serde_json::Value {
    serde_json::json!({
        "sub": "f:1234:ada",
        "preferred_username": "ada",
        "email": "ada@example.test",
        "realm_access": { "roles": ["offline_access", "default-roles-main"] },
        "resource_access": { "admin-ui": { "roles": ["admin", "editor"] } }
    })
}

#[test]
fn keycloak_reads_realm_and_client_roles() {
    let principal = ClaimsMapping::keycloak("admin-ui")
        .apply(&keycloak_token(), "keycloak")
        .unwrap();

    assert_eq!(principal.subject, "f:1234:ada");
    assert_eq!(principal.display_name.as_deref(), Some("ada"));
    assert_eq!(principal.email.as_deref(), Some("ada@example.test"));
    assert!(principal.has_role("ADMIN"), "the client role is read");
    assert!(principal.has_role("EDITOR"));
    assert!(principal.has_role("OFFLINE_ACCESS"), "so is the realm role");
}

/// Keycloak client roles *are* per-application roles, which is why the
/// principals service never needs to manage roles itself.
#[test]
fn keycloak_reads_a_different_clients_roles_when_asked() {
    let claims = serde_json::json!({
        "sub": "u",
        "resource_access": {
            "admin-ui": { "roles": ["admin"] },
            "other-app": { "roles": ["superuser"] }
        }
    });
    let principal = ClaimsMapping::keycloak("other-app")
        .apply(&claims, "keycloak")
        .unwrap();
    assert!(principal.has_role("SUPERUSER"));
    assert!(
        !principal.has_role("ADMIN"),
        "another app's roles are not ours"
    );
}

#[test]
fn authentik_reads_groups() {
    let claims = serde_json::json!({
        "sub": "u1", "preferred_username": "ada", "groups": ["admins", "staff"]
    });
    let principal = ClaimsMapping::authentik()
        .apply(&claims, "authentik")
        .unwrap();
    assert!(principal.has_role("ADMINS"));
    assert!(principal.has_role("STAFF"));
}

#[test]
fn the_auth0_namespaced_convention_works_with_or_without_a_trailing_slash() {
    let claims = serde_json::json!({
        "sub": "auth0|1", "https://example.test/roles": ["admin"]
    });
    for namespace in ["https://example.test", "https://example.test/"] {
        let principal = ClaimsMapping::namespaced(namespace)
            .apply(&claims, "auth0")
            .unwrap();
        assert!(principal.has_role("ADMIN"), "for `{namespace}`");
    }
}

/// A realm shared across applications prefixes roles; without stripping, every
/// app sees every app's roles.
#[test]
fn a_role_prefix_filters_and_strips() {
    let claims = serde_json::json!({
        "sub": "u1", "groups": ["billing:admin", "billing:viewer", "crm:admin"]
    });
    let principal = ClaimsMapping::authentik()
        .with_role_prefix("billing:")
        .apply(&claims, "authentik")
        .unwrap();

    assert!(principal.has_role("ADMIN"));
    assert!(principal.has_role("VIEWER"));
    assert_eq!(principal.roles.len(), 2, "the other app's role is not ours");
}

#[test]
fn a_single_string_role_claim_works_as_well_as_a_list() {
    let claims = serde_json::json!({"sub": "u1", "groups": "admin"});
    let principal = ClaimsMapping::authentik()
        .apply(&claims, "authentik")
        .unwrap();
    assert!(principal.has_role("ADMIN"));
}

#[test]
fn a_missing_roles_claim_is_no_roles_rather_than_an_error() {
    let claims = serde_json::json!({"sub": "u1"});
    let principal = ClaimsMapping::authentik()
        .apply(&claims, "authentik")
        .unwrap();
    assert!(principal.roles.is_empty());
}

/// A principal with no stable identity is worse than no principal.
#[test]
fn a_missing_subject_produces_nothing() {
    let claims = serde_json::json!({"name": "Ada", "groups": ["admin"]});
    assert!(
        ClaimsMapping::authentik()
            .apply(&claims, "authentik")
            .is_none()
    );
}

#[test]
fn uppercasing_can_be_turned_off_for_case_sensitive_role_names() {
    let claims = serde_json::json!({"sub": "u1", "groups": ["Admin"]});
    let principal = ClaimsMapping::authentik()
        .uppercase_roles(false)
        .apply(&claims, "authentik")
        .unwrap();
    assert!(principal.has_role("Admin"));
    assert!(!principal.has_role("ADMIN"));
}

#[test]
fn a_dotted_path_resolves_through_nested_objects() {
    use toolbox_auth::ClaimPath;
    let claims = serde_json::json!({"a": {"b": {"c": "found"}}});
    assert_eq!(ClaimPath::new("a.b.c").resolve(&claims).unwrap(), "found");
    assert!(ClaimPath::new("a.b.missing").resolve(&claims).is_none());
    assert!(ClaimPath::new("nope").resolve(&claims).is_none());
}

/// A claim *name* may itself contain dots - Auth0's namespaced convention and
/// Zitadel's URN both do - so a literal key has to win over a dotted path.
#[test]
fn a_literal_key_containing_dots_resolves_before_being_split() {
    use toolbox_auth::ClaimPath;
    let claims = serde_json::json!({
        "https://example.test/roles": ["admin"],
        "a": {"b": "nested"}
    });
    assert!(
        ClaimPath::new("https://example.test/roles")
            .resolve(&claims)
            .is_some()
    );
    assert_eq!(ClaimPath::new("a.b").resolve(&claims).unwrap(), "nested");
}
