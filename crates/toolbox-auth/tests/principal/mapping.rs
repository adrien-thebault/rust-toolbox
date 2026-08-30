use toolbox_auth::PrincipalMapping;

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
    let principal = PrincipalMapping::keycloak("admin-ui")
        .apply(&keycloak_token(), "keycloak")
        .unwrap();

    assert_eq!(principal.subject, "f:1234:ada");
    assert_eq!(principal.display_name.as_deref(), Some("ada"));
    assert_eq!(principal.email.as_deref(), Some("ada@example.test"));
    assert!(principal.has_role("ADMIN"), "the client role is read");
    assert!(principal.has_role("EDITOR"));
    assert!(principal.has_role("OFFLINE_ACCESS"), "so is the realm role");
}

/// Keycloak client roles *are* per-application roles, so a service reading
/// this mapping needs no role table of its own.
#[test]
fn keycloak_reads_a_different_clients_roles_when_asked() {
    let claims = serde_json::json!({
        "sub": "u",
        "resource_access": {
            "admin-ui": { "roles": ["admin"] },
            "other-app": { "roles": ["superuser"] }
        }
    });
    let principal = PrincipalMapping::keycloak("other-app")
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
    let principal = PrincipalMapping::authentik()
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
        let principal = PrincipalMapping::namespaced(namespace)
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
    let principal = PrincipalMapping::authentik()
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
    let principal = PrincipalMapping::authentik()
        .apply(&claims, "authentik")
        .unwrap();
    assert!(principal.has_role("ADMIN"));
}

#[test]
fn a_missing_roles_claim_is_no_roles_rather_than_an_error() {
    let claims = serde_json::json!({"sub": "u1"});
    let principal = PrincipalMapping::authentik()
        .apply(&claims, "authentik")
        .unwrap();
    assert!(principal.roles.is_empty());
}

/// A principal with no stable identity is worse than no principal.
#[test]
fn a_missing_subject_produces_nothing() {
    let claims = serde_json::json!({"name": "Ada", "groups": ["admin"]});
    assert!(
        PrincipalMapping::authentik()
            .apply(&claims, "authentik")
            .is_none()
    );
}

#[test]
fn uppercasing_can_be_turned_off_for_case_sensitive_role_names() {
    let claims = serde_json::json!({"sub": "u1", "groups": ["Admin"]});
    let principal = PrincipalMapping::authentik()
        .uppercase_roles(false)
        .apply(&claims, "authentik")
        .unwrap();
    assert!(principal.has_role("Admin"));
    assert!(!principal.has_role("ADMIN"));
}

/// An attribute is keyed by its full dotted path, not the last segment, so two
/// claims that share a leaf name do not collide.
#[test]
fn an_attribute_is_keyed_by_its_full_path() {
    let claims = serde_json::json!({
        "sub": "u1",
        "org": { "id": "acme" },
        "team": { "id": "platform" }
    });
    let principal = PrincipalMapping::authentik()
        .with_attribute("org.id")
        .with_attribute("team.id")
        .apply(&claims, "authentik")
        .unwrap();
    assert_eq!(principal.attributes["org.id"], "acme");
    assert_eq!(principal.attributes["team.id"], "platform");
}

#[test]
fn a_dotted_path_resolves_through_nested_objects() {
    use toolbox_auth::MappingPath;
    let claims = serde_json::json!({"a": {"b": {"c": "found"}}});
    assert_eq!(MappingPath::new("a.b.c").resolve(&claims).unwrap(), "found");
    assert!(MappingPath::new("a.b.missing").resolve(&claims).is_none());
    assert!(MappingPath::new("nope").resolve(&claims).is_none());
}

/// A claim *name* may itself contain dots - Auth0's namespaced convention and
/// Zitadel's URN both do - so a literal key has to win over a dotted path.
#[test]
fn a_literal_key_containing_dots_resolves_before_being_split() {
    use toolbox_auth::MappingPath;
    let claims = serde_json::json!({
        "https://example.test/roles": ["admin"],
        "a": {"b": "nested"}
    });
    assert!(
        MappingPath::new("https://example.test/roles")
            .resolve(&claims)
            .is_some()
    );
    assert_eq!(MappingPath::new("a.b").resolve(&claims).unwrap(), "nested");
}
