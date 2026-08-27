use std::sync::Arc;

use toolbox_auth::{AuthError, Principal, RefreshTokens};
use toolbox_cluster::{InMemoryKeyValue, KeyValueStore};

fn tokens() -> RefreshTokens {
    RefreshTokens::new(Arc::new(InMemoryKeyValue::default())).unwrap()
}

fn principal() -> Principal {
    Principal::new("u1", "password").with_role("ADMIN")
}

#[tokio::test]
async fn an_issued_token_can_be_redeemed_once() {
    let tokens = tokens();
    let token = tokens.issue(&principal()).await.unwrap();

    let rotated = tokens.rotate(&token).await.unwrap();
    assert_eq!(rotated.principal, principal());
    assert_ne!(rotated.token, token, "redeeming issues a different token");
}

/// The property the whole design rests on: presenting a consumed token fails,
/// which is how a leak is noticed at all.
#[tokio::test]
async fn a_replayed_token_is_refused() {
    let tokens = tokens();
    let token = tokens.issue(&principal()).await.unwrap();

    tokens.rotate(&token).await.unwrap();
    let err = tokens.rotate(&token).await.unwrap_err();
    assert_eq!(err, AuthError::Unauthenticated);
}

#[tokio::test]
async fn the_replacement_token_works_and_the_old_one_does_not() {
    let tokens = tokens();
    let first = tokens.issue(&principal()).await.unwrap();
    let second = tokens.rotate(&first).await.unwrap().token;

    assert!(tokens.rotate(&second).await.is_ok());
    assert!(tokens.rotate(&first).await.is_err());
}

#[tokio::test]
async fn an_unknown_token_is_refused() {
    assert_eq!(
        tokens().rotate("not-a-token").await.unwrap_err(),
        AuthError::Unauthenticated
    );
}

#[tokio::test]
async fn revoking_kills_the_session_within_one_access_token_lifetime() {
    let tokens = tokens();
    let token = tokens.issue(&principal()).await.unwrap();
    tokens.revoke(&token).await.unwrap();
    assert!(tokens.rotate(&token).await.is_err());
}

/// A store that cannot promise an atomic take silently permits replay, so it
/// is refused at construction rather than at runtime.
#[tokio::test]
async fn a_store_without_an_atomic_take_is_refused_at_construction() {
    struct NoTake;

    #[async_trait::async_trait]
    impl KeyValueStore for NoTake {
        fn capabilities(&self) -> toolbox_cluster::KeyValueCapabilities {
            toolbox_cluster::KeyValueCapabilities {
                atomic_take: false,
                ttl: true,
                durable: false,
                shared: false,
            }
        }
        async fn get(&self, _k: &str) -> Result<Option<Vec<u8>>, toolbox_cluster::KeyValueError> {
            Ok(None)
        }
        async fn set(
            &self,
            _k: &str,
            _v: Vec<u8>,
            _t: Option<std::time::Duration>,
        ) -> Result<(), toolbox_cluster::KeyValueError> {
            Ok(())
        }
        async fn take(&self, _k: &str) -> Result<Option<Vec<u8>>, toolbox_cluster::KeyValueError> {
            Ok(None)
        }
        async fn delete(&self, _k: &str) -> Result<(), toolbox_cluster::KeyValueError> {
            Ok(())
        }
    }

    assert!(RefreshTokens::new(Arc::new(NoTake)).is_err());
}

#[tokio::test]
async fn tokens_are_opaque_and_carry_nothing() {
    let token = tokens().issue(&principal()).await.unwrap();
    assert_eq!(token.len(), 64, "256 bits, hex");
    assert!(token.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(!token.contains("u1"), "the subject is not in the token");
}
