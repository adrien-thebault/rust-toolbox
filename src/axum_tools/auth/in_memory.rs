//! a backend that checks HTTP-Basic-style credentials against a fixed,
//! in-memory set of users, configured once at startup.

use super::{AuthBackend, AuthError, Credential, User};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// Passwords are compared as unsalted SHA-256 hex digests, not a proper
/// password hash (bcrypt/argon2/...) - no salt and a fast hash means this
/// is not resistant to offline brute-forcing, so treat it as suitable for a
/// small set of fixed operator accounts, not end-user passwords. Digests
/// are a one-liner to generate out of band (`printf '%s' 'the-password' |
/// sha256sum`).
pub struct InMemoryBasicAuthBackend {
    users: HashMap<String, (String, Vec<String>)>,
}

impl InMemoryBasicAuthBackend {
    /// `users`: username -> (sha256 hex digest of the password, roles)
    pub fn new(users: HashMap<String, (String, Vec<String>)>) -> Self {
        Self { users }
    }
}

impl AuthBackend for InMemoryBasicAuthBackend {
    fn authenticate(&self, credential: Credential) -> Result<User, AuthError> {
        let Credential::Basic { username, password } = credential;
        let (expected_hash, roles) = self
            .users
            .get(&username)
            .ok_or(AuthError::InvalidCredentials)?;

        let actual_hash = format!("{:x}", Sha256::digest(password.as_bytes()));
        if &actual_hash != expected_hash {
            return Err(AuthError::InvalidCredentials);
        }

        Ok(User {
            subject: username,
            roles: roles.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backend() -> InMemoryBasicAuthBackend {
        let mut users = HashMap::new();
        users.insert(
            "admin".to_string(),
            (
                format!("{:x}", Sha256::digest(b"hunter2")),
                vec!["admin".to_string()],
            ),
        );
        InMemoryBasicAuthBackend::new(users)
    }

    #[test]
    fn authenticates_with_correct_password() {
        let user = backend()
            .authenticate(Credential::Basic {
                username: "admin".to_string(),
                password: "hunter2".to_string(),
            })
            .expect("authenticates");
        assert_eq!(user.subject, "admin");
        assert!(user.require_role("admin").is_ok());
    }

    #[test]
    fn rejects_wrong_password() {
        let err = backend()
            .authenticate(Credential::Basic {
                username: "admin".to_string(),
                password: "wrong".to_string(),
            })
            .expect_err("rejects");
        assert!(matches!(err, AuthError::InvalidCredentials));
    }
}
