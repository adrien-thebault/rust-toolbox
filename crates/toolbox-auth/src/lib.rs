//! Identity, with no transport dependency.
//!
//! It unifies the identity type across the HTTP and gRPC boundaries. A gRPC
//! backend that validates a token must not have to compile axum to do it, which
//! is why this is its own crate rather than part of `toolbox-web`.

pub mod principal;
pub mod provider;
pub mod refresh;
pub mod session;

pub use principal::{AnyRole, AuthError, Principal, Role};
#[cfg(feature = "oidc")]
pub use provider::{AuthSession, OidcProvider};
pub use provider::{
    ClaimPath, ClaimsMapping, Credential, ForwardedIdentity, IdentityProvider, ProviderInfo,
    ProviderKind, ProviderRegistry, ProxyHeaderProvider, StoredUser, UserStore, parse_network,
};
#[cfg(feature = "password")]
pub use provider::{PasswordProvider, hash_password, verify_password};
pub use refresh::{IssuedToken, RefreshTokens};
pub use session::{Claims, JwtCodec, SessionCodec};
