//! Shared fixtures for the `auth` module tests.

use std::{
    collections::BTreeMap,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use axum::{Router, routing::get};
use secrecy::SecretString;
use toolbox_auth::{
    AuthError, JwtIdentityProvider, PasswordIdentityProvider, Principal, ProviderRegistry,
    RefreshInfo, StoredUser, UserStore, hash_password,
};
use toolbox_web::auth::{AuthState, LoginLimit, auth_router, session_layer};

mod forwarded;
mod limiter;
mod routes;
mod session;

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
