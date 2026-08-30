//! How to read a [`Principal`] out of an identity provider's claims.
//!
//! Every provider puts the subject, the roles and the display fields somewhere
//! different, and which claim carries the roles is the one thing a project
//! otherwise re-derives by reading its provider's token in a debugger. This is
//! that decision, written down once and reusable: give it the paths, hand it a
//! decoded token, get a `Principal`.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use super::Principal;

/// A dotted path into a claims document: `realm_access.roles`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MappingPath(String);

impl MappingPath {
    /// A path from its dotted form.
    ///
    /// # Arguments
    ///
    /// * `path` - The dotted path, such as `realm_access.roles`. The whole
    ///   string is also tried as a literal key, because some providers use keys
    ///   containing dots.
    pub fn new(path: impl Into<String>) -> Self {
        Self(path.into())
    }

    /// The dotted form.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Resolve this path against a claims document.
    ///
    /// The **whole string is tried as a single key first**, then as a dotted
    /// path. Both are needed: `realm_access.roles` is nested, while Auth0's
    /// `https://example.test/roles` is one literal key that happens to contain
    /// dots. Splitting unconditionally makes the second unaddressable.
    ///
    /// # Arguments
    ///
    /// * `claims` - The decoded ID token or userinfo document to walk.
    #[must_use]
    pub fn resolve<'a>(&self, claims: &'a serde_json::Value) -> Option<&'a serde_json::Value> {
        if let Some(found) = claims.get(self.0.as_str()) {
            return Some(found);
        }
        let mut current = claims;
        for segment in self.0.split('.') {
            current = current.get(segment)?;
        }
        Some(current)
    }
}

impl From<&str> for MappingPath {
    fn from(s: &str) -> Self {
        Self(s.to_owned())
    }
}

/// How to read a principal out of a provider's claims.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PrincipalMapping {
    /// Where the stable subject is. Never the email: those get reassigned.
    pub subject: MappingPath,
    /// Where the display name is.
    pub display_name: Option<MappingPath>,
    /// Where the email is.
    pub email: Option<MappingPath>,
    /// Every place roles might be. All are read and merged.
    pub roles: Vec<MappingPath>,
    /// Keep only roles starting with this, and strip it.
    ///
    /// A realm shared across applications prefixes roles with the application
    /// name; without this every app sees every app's roles.
    pub role_prefix: Option<String>,
    /// Uppercase roles after mapping, so `admin` and `ADMIN` are one role.
    pub uppercase_roles: bool,
    /// Extra claims to carry into `Principal::attributes`, each keyed by its
    /// full dotted path.
    pub attributes: Vec<MappingPath>,
}

impl Default for PrincipalMapping {
    fn default() -> Self {
        Self {
            subject: MappingPath::new("sub"),
            display_name: Some(MappingPath::new("name")),
            email: Some(MappingPath::new("email")),
            roles: Vec::new(),
            role_prefix: None,
            uppercase_roles: true,
            attributes: Vec::new(),
        }
    }
}

impl PrincipalMapping {
    /// Keycloak: realm roles and this client's roles.
    ///
    /// `resource_access.{client_id}.roles` is Keycloak's **per-application**
    /// role list, so a service reading this mapping never needs a role table of
    /// its own.
    ///
    /// # Arguments
    ///
    /// * `client_id` - The Keycloak client whose per-application roles to read,
    ///   at `resource_access.{client_id}.roles`. Realm roles are read
    ///   regardless.
    #[must_use]
    pub fn keycloak(client_id: &str) -> Self {
        Self {
            roles: vec![
                MappingPath::new("realm_access.roles"),
                MappingPath::new(format!("resource_access.{client_id}.roles")),
            ],
            display_name: Some(MappingPath::new("preferred_username")),
            ..Self::default()
        }
    }

    /// Authentik: groups.
    #[must_use]
    pub fn authentik() -> Self {
        Self {
            roles: vec![MappingPath::new("groups")],
            display_name: Some(MappingPath::new("preferred_username")),
            ..Self::default()
        }
    }

    /// The Auth0 convention: custom claims under a namespace URI.
    ///
    /// # Arguments
    ///
    /// * `namespace` - The namespace URI custom claims sit under, which Auth0
    ///   requires and which is a plain prefix on the claim name.
    #[must_use]
    pub fn namespaced(namespace: &str) -> Self {
        let namespace = namespace.trim_end_matches('/');
        Self {
            roles: vec![MappingPath::new(format!("{namespace}/roles"))],
            ..Self::default()
        }
    }

    /// Read roles from an additional path as well.
    ///
    /// # Arguments
    ///
    /// * `path` - A second claim to union roles from, for a provider that
    ///   splits them across two places.
    #[must_use]
    pub fn with_roles_at(mut self, path: impl Into<String>) -> Self {
        self.roles.push(MappingPath::new(path));
        self
    }

    /// Keep only roles with this prefix, and strip it.
    ///
    /// # Arguments
    ///
    /// * `prefix` - The prefix a role must carry to be kept. It is stripped
    ///   afterwards, so the principal holds `admin` rather than `myapp:admin`.
    #[must_use]
    pub fn with_role_prefix(mut self, prefix: impl Into<String>) -> Self {
        self.role_prefix = Some(prefix.into());
        self
    }

    /// Carry an extra claim into the principal's attributes.
    ///
    /// # Arguments
    ///
    /// * `path` - A claim to copy into the principal's attributes. Its full
    ///   dotted path becomes the attribute key.
    #[must_use]
    pub fn with_attribute(mut self, path: impl Into<String>) -> Self {
        self.attributes.push(MappingPath::new(path));
        self
    }

    /// Whether roles are uppercased.
    ///
    /// # Arguments
    ///
    /// * `uppercase` - Whether to uppercase roles. Providers disagree on case,
    ///   and a role comparison that is case-sensitive on one and not the other
    ///   is a silent authorization hole.
    #[must_use]
    pub fn uppercase_roles(mut self, uppercase: bool) -> Self {
        self.uppercase_roles = uppercase;
        self
    }

    /// Build a principal from a provider's claims.
    ///
    /// Returns `None` when the subject claim is missing, because a principal
    /// with no stable identity is worse than no principal.
    ///
    /// # Arguments
    ///
    /// * `claims` - The provider's claims document.
    /// * `issuer` - The provider id to stamp on the principal, so a later
    ///   lookup knows which provider authenticated it.
    #[must_use]
    pub fn apply(&self, claims: &serde_json::Value, issuer: &str) -> Option<Principal> {
        let subject = self.subject.resolve(claims)?.as_str()?.to_owned();

        let mut roles = BTreeSet::new();
        for path in &self.roles {
            let Some(value) = path.resolve(claims) else {
                continue;
            };
            for role in as_string_list(value) {
                if let Some(role) = self.normalize(&role) {
                    roles.insert(role);
                }
            }
        }

        let mut attributes = BTreeMap::new();
        for path in &self.attributes {
            if let Some(value) = path.resolve(claims)
                && let Some(text) = as_string(value)
            {
                attributes.insert(path.as_str().to_owned(), text);
            }
        }

        Some(Principal {
            subject,
            issuer: issuer.to_owned(),
            roles,
            display_name: self
                .display_name
                .as_ref()
                .and_then(|p| string_at(p, claims)),
            email: self.email.as_ref().and_then(|p| string_at(p, claims)),
            attributes,
        })
    }

    /// Apply the prefix filter and the case rule to one raw role.
    ///
    /// Returns `None` for a role the prefix filter excludes, and for one that
    /// is empty once the prefix is stripped.
    ///
    /// # Arguments
    ///
    /// * `role` - One raw role string, exactly as the provider spelled it.
    fn normalize(&self, role: &str) -> Option<String> {
        let role = match &self.role_prefix {
            Some(prefix) => role.strip_prefix(prefix.as_str())?,
            None => role,
        };
        if role.is_empty() {
            return None;
        }
        Some(if self.uppercase_roles {
            role.to_ascii_uppercase()
        } else {
            role.to_owned()
        })
    }
}

/// A claim may hold one role or a list of them; both are common.
///
/// # Arguments
///
/// * `value` - The claim's value: a single string, or an array of them.
fn as_string_list(value: &serde_json::Value) -> Vec<String> {
    match value {
        serde_json::Value::String(s) => vec![s.clone()],
        serde_json::Value::Array(items) => items
            .iter()
            .filter_map(|v| v.as_str().map(ToOwned::to_owned))
            .collect(),
        _ => Vec::new(),
    }
}

/// A claim as text, accepting the numbers and booleans some providers use for
/// what is conceptually a string.
///
/// # Arguments
///
/// * `value` - The claim's value.
fn as_string(value: &serde_json::Value) -> Option<String> {
    match value {
        serde_json::Value::String(s) => Some(s.clone()),
        serde_json::Value::Bool(b) => Some(b.to_string()),
        serde_json::Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

/// Follow a path into a claims document and read the result as text.
///
/// # Arguments
///
/// * `path` - Where to look.
/// * `claims` - The document to look in.
fn string_at(path: &MappingPath, claims: &serde_json::Value) -> Option<String> {
    path.resolve(claims).and_then(as_string)
}
