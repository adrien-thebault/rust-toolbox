//! An in-process gateway.

use std::net::{Ipv4Addr, SocketAddr};

use axum::{Router, extract::ConnectInfo};

use crate::problem::ProblemResponse;

/// The peer address every request appears to come from.
///
/// A real listener is served with `into_make_service_with_connect_info`, so
/// `ConnectInfo` is always present in production. The mock transport sets none,
/// which makes `client_ip` return `None` and anything keyed on it - the login
/// rate limit, an audit record - behave as if the caller were unidentifiable.
pub const TEST_PEER: SocketAddr =
    SocketAddr::new(std::net::IpAddr::V4(Ipv4Addr::LOCALHOST), 51_000);

/// A router driven in process.
///
/// No port is bound and there is no readiness wait, because there is nothing
/// to wait for: `axum-test` calls the router directly.
pub struct TestApp {
    /// The in-process axum-test server driving the router.
    server: axum_test::TestServer,
}

impl std::fmt::Debug for TestApp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("TestApp")
    }
}

impl TestApp {
    /// Drive `router` in process.
    ///
    /// # Arguments
    ///
    /// * `router` - The router to drive. It is called directly, so no port is
    ///   bound and there is nothing to wait for, and [`TEST_PEER`] stands in
    ///   for the connection a real listener would have provided.
    #[must_use]
    pub fn new(router: Router) -> Self {
        Self {
            server: axum_test::TestServer::new(with_peer(router)),
        }
    }

    /// The underlying server, for anything this wrapper does not cover.
    #[must_use]
    pub fn server(&self) -> &axum_test::TestServer {
        &self.server
    }

    /// `GET` a path.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to request, relative to the router's root.
    pub async fn get(&self, path: &str) -> axum_test::TestResponse {
        self.server.get(path).await
    }

    /// `POST` a JSON body.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to post to.
    /// * `body` - The value to serialize as the request body.
    pub async fn post_json<T: serde::Serialize>(
        &self,
        path: &str,
        body: &T,
    ) -> axum_test::TestResponse {
        self.server.post(path).json(body).await
    }

    /// `GET` a path and read the problem document it returned.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to request, expecting an RFC 9457 response.
    pub async fn get_problem(&self, path: &str) -> ProblemResponse {
        problem_of(&self.server.get(path).await)
    }

    /// `POST` a JSON body and read the problem document it returned.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to post to, expecting an RFC 9457 response.
    /// * `body` - The value to serialize as the request body.
    pub async fn post_problem<T: serde::Serialize>(&self, path: &str, body: &T) -> ProblemResponse {
        problem_of(&self.server.post(path).json(body).await)
    }
}

/// Give every request the `ConnectInfo` a real listener would have set.
///
/// # Arguments
///
/// * `router` - The router to wrap. The layer is outermost, so the address is
///   in the extensions before any extractor or limiter looks for it.
fn with_peer(router: Router) -> Router {
    router.layer(axum::middleware::from_fn(
        |mut request: axum::extract::Request, next: axum::middleware::Next| async move {
            request.extensions_mut().insert(ConnectInfo(TEST_PEER));
            next.run(request).await
        },
    ))
}

/// Read a response as a problem document.
///
/// # Arguments
///
/// * `response` - The response to decode. Its content type is captured too,
///   because serving `application/json` while claiming RFC 9457 is the bug
///   worth catching.
#[must_use]
pub fn problem_of(response: &axum_test::TestResponse) -> ProblemResponse {
    let content_type = response
        .headers()
        .get(http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    ProblemResponse::new(
        response.status_code().as_u16(),
        &content_type,
        &response.text(),
    )
}
