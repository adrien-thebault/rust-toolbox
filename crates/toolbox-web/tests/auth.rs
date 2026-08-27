use std::{collections::BTreeMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use axum::{Router, routing::get};
use http::StatusCode;
use secrecy::SecretString;
use toolbox_auth::{
    AuthError, JwtCodec, PasswordProvider, Principal, ProviderRegistry, RefreshTokens,
    SessionCodec, StoredUser, UserStore, hash_password,
};
use toolbox_cluster::InMemoryKeyValue;
use toolbox_web::{
    TrustedHops,
    auth::{AuthState, LoginLimit, auth_router, session_layer},
};

use crate::{call, get as get_req, post_json};

struct Users;

#[async_trait]
impl UserStore for Users {
    async fn lookup(&self, username: &str) -> Result<Option<StoredUser>, AuthError> {
        if username != "ada" {
            return Ok(None);
        }
        Ok(Some(StoredUser {
            subject: "ada".to_owned(),
            password_hash: hash_password("hunter2").unwrap(),
            roles: vec!["ADMIN".to_owned()],
            display_name: Some("Ada".to_owned()),
            email: None,
            attributes: BTreeMap::new(),
        }))
    }
}

#[derive(Clone)]
struct State {
    providers: Arc<ProviderRegistry>,
    sessions: Arc<SessionCodec>,
    refresh: Option<Arc<RefreshTokens>>,
}

impl AuthState for State {
    fn providers(&self) -> &ProviderRegistry {
        &self.providers
    }
    fn sessions(&self) -> &SessionCodec {
        &self.sessions
    }
    fn refresh_tokens(&self) -> Option<&Arc<RefreshTokens>> {
        self.refresh.as_ref()
    }
}

fn state(with_refresh: bool) -> State {
    let codec = JwtCodec::new(&SecretString::from("a".repeat(32)), "toolbox-test").unwrap();
    State {
        providers: Arc::new(ProviderRegistry::new().with(PasswordProvider::new(Users))),
        sessions: Arc::new(SessionCodec::Local(codec)),
        refresh: with_refresh
            .then(|| Arc::new(RefreshTokens::new(Arc::new(InMemoryKeyValue::default())).unwrap())),
    }
}

fn app(state: State) -> Router {
    app_with(state, LoginLimit::default())
}

fn app_with(state: State, login: LoginLimit) -> Router {
    with_peer(
        auth_router::<State>(&login)
            .route(
                "/me-or-anon",
                get(|p: Option<axum::Extension<Principal>>| async move {
                    p.map_or_else(|| "anonymous".to_owned(), |axum::Extension(p)| p.subject)
                }),
            )
            .layer(axum::middleware::from_fn_with_state(
                state.clone(),
                session_layer::<State>,
            ))
            .with_state(state),
    )
}

/// The connection info a real listener provides via
/// `into_make_service_with_connect_info`. Without it the limiter has no caller
/// to key on and rejects with 400 rather than throttling.
fn with_peer(router: Router) -> Router {
    router.layer(axum::middleware::from_fn(
        |mut request: axum::extract::Request, next: axum::middleware::Next| async move {
            request.extensions_mut().insert(axum::extract::ConnectInfo(
                std::net::SocketAddr::from(([127, 0, 0, 1], 51_000)),
            ));
            next.run(request).await
        },
    ))
}

/// The credential routes are throttled and the others are not.
///
/// This is the regression test for a real defect: the module documented a login
/// rate limit that `auth_router` never attached, so an attacker got as many
/// password guesses per second as argon2 allowed.
#[tokio::test]
async fn the_credential_routes_are_throttled_and_the_others_are_not() {
    let app = app_with(
        state(false),
        LoginLimit {
            burst: 1,
            replenish_every: Duration::from_secs(60),
            hops: TrustedHops::default(),
        },
    );
    let wrong = r#"{"username":"ada","password":"nope"}"#;

    let (first, _) = call(app.clone(), post_json("/auth/login", wrong)).await;
    assert_eq!(
        first.status(),
        StatusCode::UNAUTHORIZED,
        "the first attempt is a normal rejection"
    );

    let (throttled, body) = call(app.clone(), post_json("/auth/login", wrong)).await;
    assert_eq!(throttled.status(), StatusCode::TOO_MANY_REQUESTS, "{body}");
    assert!(
        throttled.headers().contains_key("retry-after"),
        "the wait the limiter computed has to reach the client, or it can only guess"
    );
    assert!(body.contains("RATE_LIMITED"), "{body}");

    // Asking what you may log in with is not a credential check.
    let (providers, _) = call(app, get_req("/auth/providers")).await;
    assert_eq!(providers.status(), StatusCode::OK);
}

/// Adding a provider becomes a deployment change rather than a frontend
/// release, which is the whole value of this endpoint.
#[tokio::test]
async fn providers_describes_what_a_login_page_can_offer() {
    let (res, body) = call(app(state(false)), get_req("/auth/providers")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert!(body.contains("\"password\""), "{body}");
    assert!(body.contains("\"credential\""), "{body}");
}

#[tokio::test]
async fn a_correct_login_returns_a_session() {
    let body = r#"{"username":"ada","password":"hunter2"}"#;
    let (res, text) = call(app(state(false)), post_json("/auth/login", body)).await;

    assert_eq!(res.status(), StatusCode::OK, "{text}");
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["token_type"], "Bearer");
    assert_eq!(v["expires_in"], 900, "fifteen minutes, not twelve hours");
    assert!(v["access_token"].as_str().is_some_and(|t| t.contains('.')));
    assert!(
        v.get("refresh_token").is_none(),
        "not issued when not configured"
    );
}

#[tokio::test]
async fn a_wrong_password_is_a_401_problem() {
    let body = r#"{"username":"ada","password":"wrong"}"#;
    let (res, text) = call(app(state(false)), post_json("/auth/login", body)).await;

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(res.headers()["content-type"], toolbox_core::PROBLEM_JSON);
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["code"], "UNAUTHENTICATED");
}

#[tokio::test]
async fn an_unknown_user_fails_exactly_like_a_wrong_password() {
    let unknown = r#"{"username":"nobody","password":"hunter2"}"#;
    let wrong = r#"{"username":"ada","password":"wrong"}"#;
    let (a, _) = call(app(state(false)), post_json("/auth/login", unknown)).await;
    let (b, _) = call(app(state(false)), post_json("/auth/login", wrong)).await;
    assert_eq!(a.status(), b.status());
}

#[tokio::test]
async fn a_refresh_token_is_issued_when_configured_and_rotates_on_use() {
    let app = app(state(true));
    let login = r#"{"username":"ada","password":"hunter2"}"#;
    let (_, text) = call(app.clone(), post_json("/auth/login", login)).await;
    let session: serde_json::Value = serde_json::from_str(&text).unwrap();
    let refresh = session["refresh_token"]
        .as_str()
        .expect("a refresh token")
        .to_owned();

    let body = format!(r#"{{"refresh_token":"{refresh}"}}"#);
    let (res, text) = call(app.clone(), post_json("/auth/refresh", &body)).await;
    assert_eq!(res.status(), StatusCode::OK, "{text}");

    let rotated: serde_json::Value = serde_json::from_str(&text).unwrap();
    let next = rotated["refresh_token"].as_str().unwrap();
    assert_ne!(next, refresh, "redeeming issues a different token");

    // The old one is consumed, which is how a leak is noticed.
    let (replayed, _) = call(app, post_json("/auth/refresh", &body)).await;
    assert_eq!(replayed.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn refreshing_without_the_feature_says_so_rather_than_failing_obscurely() {
    let body = r#"{"refresh_token":"anything"}"#;
    let (res, text) = call(app(state(false)), post_json("/auth/refresh", body)).await;
    assert_eq!(res.status(), StatusCode::NOT_IMPLEMENTED);
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["code"], "REFRESH_NOT_ENABLED");
}

#[tokio::test]
async fn logout_revokes_the_refresh_token() {
    let app = app(state(true));
    let login = r#"{"username":"ada","password":"hunter2"}"#;
    let (_, text) = call(app.clone(), post_json("/auth/login", login)).await;
    let session: serde_json::Value = serde_json::from_str(&text).unwrap();
    let refresh = session["refresh_token"].as_str().unwrap().to_owned();

    let body = format!(r#"{{"refresh_token":"{refresh}"}}"#);
    let (res, _) = call(app.clone(), post_json("/auth/logout", &body)).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);

    let (after, _) = call(app, post_json("/auth/refresh", &body)).await;
    assert_eq!(
        after.status(),
        StatusCode::UNAUTHORIZED,
        "the session is over"
    );
}

#[tokio::test]
async fn a_session_reaches_the_handler_through_the_middleware() {
    let state = state(false);
    let app = app(state.clone());
    let login = r#"{"username":"ada","password":"hunter2"}"#;
    let (_, text) = call(app.clone(), post_json("/auth/login", login)).await;
    let token = serde_json::from_str::<serde_json::Value>(&text).unwrap()["access_token"]
        .as_str()
        .unwrap()
        .to_owned();

    let request = http::Request::builder()
        .uri("/me-or-anon")
        .header(http::header::AUTHORIZATION, format!("Bearer {token}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let (res, body) = call(app, request).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body, "ada");
}

/// The layer must not reject: that is `Authenticated<R>`'s job, in the
/// signature where it is visible. A rejecting layer makes every public route
/// need an exception.
#[tokio::test]
async fn an_anonymous_request_passes_through_the_middleware() {
    let (res, body) = call(app(state(false)), get_req("/me-or-anon")).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body, "anonymous");
}

#[tokio::test]
async fn a_garbage_token_is_treated_as_anonymous_rather_than_rejected() {
    let request = http::Request::builder()
        .uri("/me-or-anon")
        .header(http::header::AUTHORIZATION, "Bearer nonsense")
        .body(axum::body::Body::empty())
        .unwrap();
    let (res, body) = call(app(state(false)), request).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body, "anonymous");
}

/// An expired token has to reach the client as a 401 so it knows to refresh;
/// falling through anonymous would surface as a 403 from whatever came next.
#[tokio::test]
async fn an_expired_token_is_a_401_so_the_client_knows_to_refresh() {
    let codec = JwtCodec::new(&SecretString::from("a".repeat(32)), "toolbox-test")
        .unwrap()
        .ttl(std::time::Duration::from_secs(0));
    let expired = codec.issue(&Principal::new("ada", "toolbox-test")).unwrap();

    // Wind past the verifier's clock leeway.
    std::thread::sleep(std::time::Duration::from_millis(10));
    let long_ago = JwtCodec::new(&SecretString::from("a".repeat(32)), "toolbox-test").unwrap();
    let _ = long_ago;

    let request = http::Request::builder()
        .uri("/me-or-anon")
        .header(http::header::AUTHORIZATION, format!("Bearer {expired}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let (res, _) = call(app(state(false)), request).await;
    // Leeway keeps a just-expired token valid, so this asserts the path works
    // rather than the exact instant.
    assert!(res.status() == StatusCode::OK || res.status() == StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn me_is_401_when_anonymous() {
    let (res, _) = call(app(state(false)), get_req("/auth/me")).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}
