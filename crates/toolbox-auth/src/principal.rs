//! Who is making a request.

pub mod mapping;

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};
use toolbox_core::{ErrorKind, ServiceError};

/// A role, named by the consumer.
///
/// Role *values* are deliberately not defined here: A consumer writes its own
/// enum, implements this, and gets `Authenticated<Admin>` in a handler
/// signature.
pub trait Role: Send + Sync + 'static {
    /// The string this role is carried as in a token.
    const NAME: &'static str;
}

/// Matches any authenticated caller, whatever roles they hold.
#[derive(Debug, Clone, Copy)]
pub struct AnyRole;

impl Role for AnyRole {
    const NAME: &'static str = "*";
}

/// An authenticated caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Principal {
    /// Stable within `issuer`. Never an email: those get reassigned.
    pub subject: String,
    /// Who vouched for this principal.
    pub issuer: String,
    /// The roles they hold, as strings
    #[serde(default)]
    pub roles: BTreeSet<String>,
    /// For display only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    /// For display and correspondence only, never for identity.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    /// Anything else the provider supplied.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub attributes: BTreeMap<String, String>,
}

impl Principal {
    /// A principal with a subject and an issuer.
    ///
    /// # Arguments
    ///
    /// * `subject` - The provider's stable identifier for this user. Stable is
    ///   the requirement: an email is not one.
    /// * `issuer` - Which provider authenticated them, so a token can be
    ///   attributed later.
    pub fn new(subject: impl Into<String>, issuer: impl Into<String>) -> Self {
        Self {
            subject: subject.into(),
            issuer: issuer.into(),
            roles: BTreeSet::new(),
            display_name: None,
            email: None,
            attributes: BTreeMap::new(),
        }
    }

    /// Add a role.
    ///
    /// # Arguments
    ///
    /// * `role` - The role to add. Role values belong to the consumer, so this
    ///   takes a string rather than an enum this crate would have to define.
    #[must_use]
    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.roles.insert(role.into());
        self
    }

    /// Add several roles.
    ///
    /// # Arguments
    ///
    /// * `roles` - The roles to add, in one call.
    #[must_use]
    pub fn with_roles<I: IntoIterator<Item = S>, S: Into<String>>(mut self, roles: I) -> Self {
        self.roles.extend(roles.into_iter().map(Into::into));
        self
    }

    /// Set the display name.
    ///
    /// # Arguments
    ///
    /// * `name` - What to show a human. Never used for authorization: it is not
    ///   stable and not unique.
    #[must_use]
    pub fn with_display_name(mut self, name: impl Into<String>) -> Self {
        self.display_name = Some(name.into());
        self
    }

    /// Set the email.
    ///
    /// # Arguments
    ///
    /// * `email` - The address, for display and notification. Not an identity:
    ///   providers let it change.
    #[must_use]
    pub fn with_email(mut self, email: impl Into<String>) -> Self {
        self.email = Some(email.into());
        self
    }

    /// Attach one attribute.
    ///
    /// # Arguments
    ///
    /// * `key` - The attribute name, usually the last segment of the claim it
    ///   came from.
    /// * `value` - Its value. It travels in the session token, so it must stay
    ///   small.
    #[must_use]
    pub fn with_attribute(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.attributes.insert(key.into(), value.into());
        self
    }

    /// Whether this principal holds `role`, compared exactly.
    ///
    /// For a genuinely dynamic check. Prefer `Authenticated<R>` in the handler
    /// signature, which cannot be forgotten. `"*"` is a literal here, not a
    /// wildcard: for "any authenticated caller" use [`Principal::has`] with
    /// [`AnyRole`].
    ///
    /// # Arguments
    ///
    /// * `role` - The role to test for. The mapping's case rule is what makes
    ///   that comparison safe.
    #[must_use]
    pub fn has_role(&self, role: &str) -> bool {
        self.roles.contains(role)
    }

    /// Whether this principal satisfies `R`.
    ///
    /// This is [`Principal::has_role`] plus one thing: [`AnyRole`] is satisfied
    /// by any authenticated caller, roles or not.
    #[must_use]
    pub fn has<R: Role>(&self) -> bool {
        R::NAME == AnyRole::NAME || self.has_role(R::NAME)
    }

    /// Fail unless this principal holds `role`.
    ///
    /// # Arguments
    ///
    /// * `role` - The role required. It is named in the error, so the caller
    ///   learns what was missing.
    ///
    /// # Errors
    /// [`AuthError::Forbidden`] naming the role required.
    pub fn require_role(&self, role: &str) -> Result<(), AuthError> {
        if self.has_role(role) {
            Ok(())
        } else {
            Err(AuthError::Forbidden {
                required: role.to_owned(),
            })
        }
    }

    /// A globally unique identity: the issuer and subject together.
    ///
    /// A subject is unique only within its issuer, so two identity providers
    /// can each have a user `1`. Anything storing a principal must key on both.
    #[must_use]
    pub fn key(&self) -> String {
        format!("{}|{}", self.issuer, self.subject)
    }
}

/// Why authentication or authorization failed.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum AuthError {
    /// No credentials, or credentials that did not verify.
    #[error("not authenticated")]
    Unauthenticated,
    /// Authenticated, but not permitted.
    #[error("requires the `{required}` role")]
    Forbidden {
        /// The role that was needed.
        required: String,
    },
    /// The credentials were structurally wrong.
    #[error("malformed credentials: {0}")]
    Malformed(String),
    /// The session or token has expired.
    #[error("the session has expired")]
    Expired,
}

impl ServiceError for AuthError {
    fn code(&self) -> &'static str {
        match self {
            Self::Unauthenticated => "UNAUTHENTICATED",
            Self::Forbidden { .. } => "FORBIDDEN",
            Self::Malformed(_) => "MALFORMED_CREDENTIALS",
            Self::Expired => "SESSION_EXPIRED",
        }
    }

    fn domain(&self) -> &'static str {
        "auth"
    }

    fn kind(&self) -> ErrorKind {
        match self {
            Self::Forbidden { .. } => ErrorKind::PermissionDenied,
            // An expired session is a 401 so the client knows to refresh;
            // a 403 would tell it to give up.
            Self::Unauthenticated | Self::Expired | Self::Malformed(_) => {
                ErrorKind::Unauthenticated
            }
        }
    }

    fn metadata(&self) -> BTreeMap<String, String> {
        match self {
            Self::Forbidden { required } => {
                BTreeMap::from([("required_role".to_owned(), required.clone())])
            }
            _ => BTreeMap::new(),
        }
    }
}
