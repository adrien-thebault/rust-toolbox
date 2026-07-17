//! tests `src/axum_tools/auth.rs`: the `User` extractor end-to-end -
//! header parsing (including scheme case-insensitivity), session decoding,
//! and the `ApiError` rejections.

use axum::extract::FromRequestParts;
use axum::http::{Request, header::AUTHORIZATION, request::Parts};
use rust_toolbox::axum_tools::api_error::ApiError;
use rust_toolbox::axum_tools::auth::{
    Claims, DecodingKey, EncodingKey, JwtCodec, JwtCodecProvider, User,
};

struct TestState {
    codec: JwtCodec,
}

impl JwtCodecProvider for TestState {
    fn jwt_codec(&self) -> &JwtCodec {
        &self.codec
    }
}

fn state() -> TestState {
    TestState {
        codec: JwtCodec::new(
            EncodingKey::from_secret(b"secret"),
            DecodingKey::from_secret(b"secret"),
        ),
    }
}

fn user() -> User {
    User {
        subject: "admin".to_string(),
        roles: vec!["admin".to_string()],
    }
}

fn token(state: &TestState) -> String {
    state
        .codec
        .encode(&Claims::for_user(&user(), chrono::Duration::minutes(5)))
        .expect("signs")
}

fn parts(authorization: Option<&str>) -> Parts {
    let mut builder = Request::builder().uri("/");
    if let Some(value) = authorization {
        builder = builder.header(AUTHORIZATION, value);
    }
    builder.body(()).expect("request").into_parts().0
}

#[tokio::test]
async fn extracts_the_user_from_a_valid_bearer_token() {
    let state = state();
    let mut parts = parts(Some(&format!("Bearer {}", token(&state))));

    let extracted = User::from_request_parts(&mut parts, &state)
        .await
        .expect("a valid session extracts");
    assert_eq!(extracted, user());
}

#[tokio::test]
async fn the_auth_scheme_is_case_insensitive() {
    let state = state();
    let mut parts = parts(Some(&format!("bearer {}", token(&state))));

    assert!(User::from_request_parts(&mut parts, &state).await.is_ok());
}

#[tokio::test]
async fn a_missing_header_is_unauthenticated() {
    let state = state();
    let mut parts = parts(None);

    let rejection = User::from_request_parts(&mut parts, &state)
        .await
        .expect_err("no header, no session");
    assert!(matches!(rejection, ApiError::Unauthenticated));
}

#[tokio::test]
async fn a_non_bearer_scheme_is_unauthenticated() {
    let state = state();
    let mut parts = parts(Some("Basic YWRtaW46aHVudGVyMg=="));

    let rejection = User::from_request_parts(&mut parts, &state)
        .await
        .expect_err("only bearer tokens are sessions");
    assert!(matches!(rejection, ApiError::Unauthenticated));
}

#[tokio::test]
async fn a_garbage_token_is_unauthenticated() {
    let state = state();
    let mut parts = parts(Some("Bearer not-a-jwt"));

    let rejection = User::from_request_parts(&mut parts, &state)
        .await
        .expect_err("invalid tokens are rejected");
    assert!(matches!(rejection, ApiError::Unauthenticated));
}

#[tokio::test]
async fn a_token_signed_with_another_key_is_unauthenticated() {
    let state = state();
    let other = TestState {
        codec: JwtCodec::new(
            EncodingKey::from_secret(b"other"),
            DecodingKey::from_secret(b"other"),
        ),
    };
    let mut parts = parts(Some(&format!("Bearer {}", token(&other))));

    let rejection = User::from_request_parts(&mut parts, &state)
        .await
        .expect_err("wrong key, no session");
    assert!(matches!(rejection, ApiError::Unauthenticated));
}
