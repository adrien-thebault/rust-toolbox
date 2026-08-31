//! Identity, with no transport dependency.
//!
//! It unifies the identity type across the HTTP and gRPC boundaries. A backend
//! that validates a token must not have to compile axum - or the cluster traits
//! - to do it, which is why this is its own crate.

pub mod principal;
pub mod provider;

pub use principal::{
    AnyRole, AuthError, Principal, Role,
    mapping::{MappingPath, PrincipalMapping},
};
#[cfg(feature = "password")]
pub use provider::password::{
    PasswordIdentityProvider, StoredUser, UserStore, auth_epoch, hash_password, verify_password,
};
pub use provider::{
    Credential, IdentityProvider, ProviderRegistry, constant_time_eq,
    forwarded_principal::{ForwardedPrincipal, ForwardedPrincipalProvider},
    jwt::{Claims, JwtIdentityProvider, RefreshInfo, Refreshed, TokenUse},
    proxy_header::{ForwardedHeaders, ForwardedIdentity, ForwardedIdentityProvider, parse_network},
};
