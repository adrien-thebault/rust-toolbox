//! The trait's default methods: the stateless baseline a minimal implementor
//! gets without overriding anything.

use std::sync::Arc;

use secrecy::SecretString;
use toolbox_auth::{JwtIdentityProvider, Principal, ProviderRegistry, RefreshInfo};
use toolbox_web::auth::AuthState;

#[derive(Clone)]
struct Minimal {
    providers: Arc<ProviderRegistry>,
    issuer: Arc<JwtIdentityProvider>,
}

impl AuthState for Minimal {
    fn providers(&self) -> &ProviderRegistry {
        &self.providers
    }
    fn session_issuer(&self) -> &JwtIdentityProvider {
        &self.issuer
    }
}

fn minimal() -> Minimal {
    let issuer = Arc::new(
        JwtIdentityProvider::hmac(&SecretString::from("a".repeat(32)), "toolbox-test").unwrap(),
    );
    Minimal {
        providers: Arc::new(ProviderRegistry::new().with_arc(issuer.clone())),
        issuer,
    }
}

#[tokio::test]
async fn the_default_refresh_epoch_binds_to_nothing() {
    let principal = Principal::new("ada", "toolbox-test");
    assert_eq!(minimal().refresh_epoch(&principal).await, None);
}

#[tokio::test]
async fn the_default_resolve_refresh_trusts_the_token_as_is() {
    let stale = Principal::new("ada", "toolbox-test");
    let info = RefreshInfo {
        subject: "ada".to_owned(),
        idp: "toolbox-test".to_owned(),
        epoch: None,
        stale: stale.clone(),
    };
    assert_eq!(minimal().resolve_refresh(info).await.unwrap(), stale);
}
