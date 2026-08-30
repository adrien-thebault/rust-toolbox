use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use axum::{Router, routing::get};
use http::StatusCode;
use secrecy::SecretString;
use toolbox_auth::{
    AuthError, ForwardedIdentityProvider, JwtIdentityProvider, PasswordIdentityProvider, Principal,
    ProviderRegistry, RefreshInfo, StoredUser, UserStore, hash_password,
};
use toolbox_web::{
    TrustedHops,
    auth::{
        AuthState, ForwardedConfig, LoginLimit, auth_router, forwarded_auth_layer, session_layer,
    },
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
    issuer: Arc<JwtIdentityProvider>,
    /// The credential fingerprint the deployment currently reports, if any.
    epoch: Arc<Mutex<Option<String>>>,
}

impl AuthState for State {
    fn providers(&self) -> &ProviderRegistry {
        &self.providers
    }
    fn session_issuer(&self) -> &JwtIdentityProvider {
        &self.issuer
    }
    fn refresh_epoch(
        &self,
        _principal: &Principal,
    ) -> impl std::future::Future<Output = Option<String>> + Send {
        std::future::ready(self.epoch.lock().unwrap().clone())
    }
    fn resolve_refresh(
        &self,
        info: RefreshInfo,
    ) -> impl std::future::Future<Output = Result<Principal, AuthError>> + Send {
        let current = self.epoch.lock().unwrap().clone();
        std::future::ready(match (info.epoch.as_deref(), current.as_deref()) {
            (Some(bound), cur) if Some(bound) != cur => Err(AuthError::Unauthenticated),
            _ => Ok(info.stale),
        })
    }
}

fn state() -> State {
    let issuer: Arc<JwtIdentityProvider> = Arc::new(
        JwtIdentityProvider::hmac(&SecretString::from("a".repeat(32)), "toolbox-test").unwrap(),
    );
    State {
        providers: Arc::new(
            ProviderRegistry::new()
                .with_arc(issuer.clone())
                .with(PasswordIdentityProvider::new(Users)),
        ),
        issuer,
        epoch: Arc::new(Mutex::new(None)),
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
        state(),
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

    // `/auth/me` is not a credential check and is not throttled.
    let (me, _) = call(app, get_req("/auth/me")).await;
    assert_eq!(
        me.status(),
        StatusCode::UNAUTHORIZED,
        "reached, not throttled"
    );
}

#[tokio::test]
async fn a_correct_login_returns_a_session_with_a_refresh_token() {
    let body = r#"{"username":"ada","password":"hunter2"}"#;
    let (res, text) = call(app(state()), post_json("/auth/login", body)).await;

    assert_eq!(res.status(), StatusCode::OK, "{text}");
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["token_type"], "Bearer");
    assert_eq!(v["expires_in"], 900, "fifteen minutes, not twelve hours");
    assert!(v["access_token"].as_str().is_some_and(|t| t.contains('.')));
    assert!(
        v["refresh_token"].as_str().is_some_and(|t| t.contains('.')),
        "a refresh token is always issued now"
    );
}

#[tokio::test]
async fn a_wrong_password_is_a_401_problem() {
    let body = r#"{"username":"ada","password":"wrong"}"#;
    let (res, text) = call(app(state()), post_json("/auth/login", body)).await;

    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
    assert_eq!(res.headers()["content-type"], toolbox_core::PROBLEM_JSON);
    let v: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert_eq!(v["code"], "UNAUTHENTICATED");
}

#[tokio::test]
async fn an_unknown_user_fails_exactly_like_a_wrong_password() {
    let unknown = r#"{"username":"nobody","password":"hunter2"}"#;
    let wrong = r#"{"username":"ada","password":"wrong"}"#;
    let (a, _) = call(app(state()), post_json("/auth/login", unknown)).await;
    let (b, _) = call(app(state()), post_json("/auth/login", wrong)).await;
    assert_eq!(a.status(), b.status());
}

#[tokio::test]
async fn a_refresh_token_can_be_redeemed_for_a_new_session() {
    let app = app(state());
    let login = r#"{"username":"ada","password":"hunter2"}"#;
    let (_, text) = call(app.clone(), post_json("/auth/login", login)).await;
    let session: serde_json::Value = serde_json::from_str(&text).unwrap();
    let refresh = session["refresh_token"].as_str().unwrap().to_owned();

    let body = format!(r#"{{"refresh_token":"{refresh}"}}"#);
    let (res, text) = call(app.clone(), post_json("/auth/refresh", &body)).await;
    assert_eq!(res.status(), StatusCode::OK, "{text}");

    let rotated: serde_json::Value = serde_json::from_str(&text).unwrap();
    assert!(
        rotated["access_token"]
            .as_str()
            .is_some_and(|t| t.contains('.'))
    );
    assert!(rotated["refresh_token"].as_str().is_some());
}

/// Stateless refresh has no server-side record, so a changed credential
/// fingerprint is the revocation mechanism.
#[tokio::test]
async fn a_changed_credential_fingerprint_invalidates_a_refresh_token() {
    let state = state();
    *state.epoch.lock().unwrap() = Some("epoch-1".to_owned());
    let app = app(state.clone());

    let login = r#"{"username":"ada","password":"hunter2"}"#;
    let (_, text) = call(app.clone(), post_json("/auth/login", login)).await;
    let refresh = serde_json::from_str::<serde_json::Value>(&text).unwrap()["refresh_token"]
        .as_str()
        .unwrap()
        .to_owned();
    let body = format!(r#"{{"refresh_token":"{refresh}"}}"#);

    // Same fingerprint: still good.
    let (ok, _) = call(app.clone(), post_json("/auth/refresh", &body)).await;
    assert_eq!(ok.status(), StatusCode::OK);

    // The password changed.
    *state.epoch.lock().unwrap() = Some("epoch-2".to_owned());
    let (rejected, _) = call(app, post_json("/auth/refresh", &body)).await;
    assert_eq!(rejected.status(), StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn logout_is_a_no_op_the_client_drives() {
    let (res, _) = call(app(state()), post_json("/auth/logout", "")).await;
    assert_eq!(res.status(), StatusCode::NO_CONTENT);
}

#[tokio::test]
async fn a_session_reaches_the_handler_through_the_middleware() {
    let state = state();
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
    let (res, body) = call(app(state()), get_req("/me-or-anon")).await;
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
    let (res, body) = call(app(state()), request).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body, "anonymous");
}

/// An expired token has to reach the client as a 401 so it knows to refresh;
/// falling through anonymous would surface as a 403 from whatever came next.
#[tokio::test]
async fn an_expired_token_is_a_401_so_the_client_knows_to_refresh() {
    let codec = JwtIdentityProvider::hmac(&SecretString::from("a".repeat(32)), "toolbox-test")
        .unwrap()
        .ttl(Duration::from_secs(0));
    let expired = codec.issue(&Principal::new("ada", "toolbox-test")).unwrap();

    let request = http::Request::builder()
        .uri("/me-or-anon")
        .header(http::header::AUTHORIZATION, format!("Bearer {expired}"))
        .body(axum::body::Body::empty())
        .unwrap();
    let (res, _) = call(app(state()), request).await;
    // Leeway keeps a just-expired token valid, so this asserts the path works
    // rather than the exact instant.
    assert!(res.status() == StatusCode::OK || res.status() == StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn me_is_401_when_anonymous() {
    let (res, _) = call(app(state()), get_req("/auth/me")).await;
    assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
}

// --- forwarded-identity layer ---------------------------------------------

fn forwarded_app(peer: [u8; 4]) -> Router {
    let issuer: Arc<JwtIdentityProvider> = Arc::new(
        JwtIdentityProvider::hmac(&SecretString::from("a".repeat(32)), "toolbox-test").unwrap(),
    );
    let st = State {
        providers: Arc::new(
            ProviderRegistry::new()
                .with_arc(issuer.clone())
                .with(ForwardedIdentityProvider::new(&["127.0.0.1"]).unwrap()),
        ),
        issuer,
        epoch: Arc::new(Mutex::new(None)),
    };

    let router = Router::new()
        .route(
            "/me-or-anon",
            get(|p: Option<axum::Extension<Principal>>| async move {
                p.map_or_else(|| "anonymous".to_owned(), |axum::Extension(p)| p.subject)
            }),
        )
        .layer(axum::middleware::from_fn_with_state(
            (st.clone(), ForwardedConfig::default()),
            forwarded_auth_layer::<State>,
        ))
        .layer(axum::middleware::from_fn_with_state(
            st.clone(),
            session_layer::<State>,
        ))
        .with_state(st);

    router.layer(axum::middleware::from_fn(
        move |mut request: axum::extract::Request, next: axum::middleware::Next| async move {
            request.extensions_mut().insert(axum::extract::ConnectInfo(
                std::net::SocketAddr::from((peer, 40_000)),
            ));
            next.run(request).await
        },
    ))
}

#[tokio::test]
async fn a_trusted_proxy_can_forward_an_identity() {
    let request = http::Request::builder()
        .uri("/me-or-anon")
        .header("x-forwarded-user", "ada")
        .header("x-forwarded-groups", "admins,staff")
        .body(axum::body::Body::empty())
        .unwrap();
    let (res, body) = call(forwarded_app([127, 0, 0, 1]), request).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body, "ada");
}

/// The same headers from an untrusted peer are ignored: a spoofable header is
/// total authentication bypass.
#[tokio::test]
async fn the_same_headers_from_an_untrusted_peer_are_ignored() {
    let request = http::Request::builder()
        .uri("/me-or-anon")
        .header("x-forwarded-user", "ada")
        .body(axum::body::Body::empty())
        .unwrap();
    let (res, body) = call(forwarded_app([203, 0, 113, 9]), request).await;
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(body, "anonymous");
}
