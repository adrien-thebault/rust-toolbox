use std::sync::{Arc, Mutex};

use axum::{Router, routing::get};
use http::StatusCode;
use secrecy::SecretString;
use toolbox_auth::{ForwardedIdentityProvider, JwtIdentityProvider, Principal, ProviderRegistry};
use toolbox_web::auth::{ForwardedConfig, forwarded_auth_layer, session_layer};

use super::State;
use crate::call;

fn forwarded_app(peer: [u8; 4]) -> Router {
    let issuer: Arc<JwtIdentityProvider> = Arc::new(
        JwtIdentityProvider::hmac(&SecretString::from("a".repeat(32)), "toolbox-test").unwrap(),
    );
    let forwarded = ForwardedIdentityProvider::trusting_peers(&["127.0.0.1"]).unwrap();
    let config = ForwardedConfig::for_provider(&forwarded);
    let st = State {
        providers: Arc::new(
            ProviderRegistry::new()
                .with_arc(issuer.clone())
                .with(forwarded),
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
            (st.clone(), config),
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
