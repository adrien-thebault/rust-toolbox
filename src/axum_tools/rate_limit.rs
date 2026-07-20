//! per-IP request throttling for axum handlers worth protecting from abuse
//! (login, public write endpoints, ...), built on [`tower_governor`] (a
//! keyed rate limiter over the `governor` crate) rather than hand-rolled.

use crate::axum_tools::api_error::ApiError;
use axum::extract::connect_info::ConnectInfo;
use axum::http::{HeaderMap, HeaderName, Request};
use axum::response::{IntoResponse, Response};
use std::net::{IpAddr, SocketAddr};
use tower_governor::{GovernorError, key_extractor::KeyExtractor};

/// the real client IP for a request that reached this service through a
/// reverse proxy trusted to append to `X-Forwarded-For` (nginx, Caddy, an
/// ALB, ...): the last (rightmost) entry is the one appended by the proxy
/// sitting directly in front of this service - trusted only because that
/// proxy is this service's sole entry point, so nothing else can reach it
/// directly to forge an earlier entry. Falls back to the raw TCP peer
/// address when the header is absent or unparseable (e.g. a request
/// reaching this service directly, bypassing the proxy - typical during
/// local development). Used by [`ForwardedForKeyExtractor`], and public so
/// a handler that wants to log or act on the real visitor IP itself (not
/// just rate-limit by it) can call it directly.
pub fn resolve_client_ip(headers: &HeaderMap, peer: SocketAddr) -> IpAddr {
    headers
        .get(HeaderName::from_static("x-forwarded-for"))
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.rsplit(',').next())
        .and_then(|ip| ip.trim().parse::<IpAddr>().ok())
        .unwrap_or_else(|| peer.ip())
}

/// keys a [`tower_governor`] rate limiter by [`resolve_client_ip`]'s real
/// client IP - deliberately not `tower_governor`'s own `SmartIpKeyExtractor`,
/// which trusts the *leftmost* `X-Forwarded-For` entry: that entry is
/// client-supplied and unverified, whereas only the *rightmost* one is the
/// one the trusted reverse proxy in front of this service actually appends
/// (see [`resolve_client_ip`]) - trusting the leftmost one would let an attacker
/// dodge the limit just by sending a fake header. Requires the service to
/// be served via `axum::serve(...).into_make_service_with_connect_info::<SocketAddr>()`
/// (or the equivalent tonic/hyper setup) so [`ConnectInfo`] is present on
/// every request's extensions.
#[derive(Debug, Clone, Copy)]
pub struct ForwardedForKeyExtractor;

impl KeyExtractor for ForwardedForKeyExtractor {
    type Key = IpAddr;

    fn extract<T>(&self, req: &Request<T>) -> Result<Self::Key, GovernorError> {
        let peer = req
            .extensions()
            .get::<ConnectInfo<SocketAddr>>()
            .map(|info| info.0)
            .ok_or(GovernorError::UnableToExtractKey)?;
        Ok(resolve_client_ip(req.headers(), peer))
    }
}

/// with [`ForwardedForKeyExtractor`], only [`GovernorError::TooManyRequests`]
/// is ever actually constructed in practice (extraction only fails when
/// [`ConnectInfo`] is missing from the request's extensions, which a
/// correctly-configured server never omits), so anything else maps to
/// [`ApiError::Internal`] as a safe, generic fallback. Lives here (rather
/// than alongside `api_error`'s other `From<_> for ApiError` impls) so
/// `api_error.rs` itself stays free of any `tower_governor` awareness -
/// this whole module is already gated behind the `rate-limit` feature, and
/// Rust's orphan rules only require the impl to live somewhere in this
/// crate, not specifically next to [`ApiError`]'s definition.
impl From<GovernorError> for ApiError {
    fn from(err: GovernorError) -> Self {
        match err {
            GovernorError::TooManyRequests { .. } => Self::ResourceExhausted,
            GovernorError::UnableToExtractKey | GovernorError::Other { .. } => Self::Internal,
        }
    }
}

/// `tower_governor`'s own default error response is a bare, plain-text body
/// ("Too Many Requests! Wait for Ns") - pass this to a rate-limited route's
/// `GovernorLayer::error_handler` so a throttled request gets the same
/// [`ApiError`]/`application/problem+json` shape as every other error this
/// gateway returns, rather than being the one exception. A thin wrapper
/// around `ApiError::from(err).into_response()` so call sites can pass this
/// function by name instead of repeating that conversion at every
/// `.error_handler(...)` call.
pub fn error_response_handler(err: GovernorError) -> Response {
    ApiError::from(err).into_response()
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{HeaderValue, StatusCode};

    fn peer() -> SocketAddr {
        "203.0.113.9:12345".parse().expect("valid socket addr")
    }

    #[test]
    fn resolve_client_ip_falls_back_to_the_tcp_peer_without_a_forwarded_for_header() {
        assert_eq!(resolve_client_ip(&HeaderMap::new(), peer()), peer().ip());
    }

    #[test]
    fn resolve_client_ip_trusts_the_rightmost_entry_not_the_client_supplied_leftmost_one() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("x-forwarded-for"),
            HeaderValue::from_static("198.51.100.7, 203.0.113.42"),
        );
        assert_eq!(
            resolve_client_ip(&headers, peer()),
            "203.0.113.42".parse::<IpAddr>().expect("valid ip")
        );
    }

    #[test]
    fn resolve_client_ip_falls_back_to_the_peer_for_an_unparseable_header() {
        let mut headers = HeaderMap::new();
        headers.insert(
            HeaderName::from_static("x-forwarded-for"),
            HeaderValue::from_static("not-an-ip"),
        );
        assert_eq!(resolve_client_ip(&headers, peer()), peer().ip());
    }

    #[test]
    fn extract_reads_the_real_client_ip_from_connect_info() {
        let mut req = Request::builder()
            .body(Body::empty())
            .expect("valid request");
        req.extensions_mut().insert(ConnectInfo(peer()));

        let key = ForwardedForKeyExtractor
            .extract(&req)
            .expect("key extracted");
        assert_eq!(key, peer().ip());
    }

    #[test]
    fn extract_fails_without_connect_info() {
        let req = Request::builder()
            .body(Body::empty())
            .expect("valid request");
        assert!(matches!(
            ForwardedForKeyExtractor.extract(&req),
            Err(GovernorError::UnableToExtractKey)
        ));
    }

    #[test]
    fn too_many_requests_converts_to_resource_exhausted() {
        let api_error = ApiError::from(GovernorError::TooManyRequests {
            wait_time: 1,
            headers: None,
        });
        assert!(matches!(api_error, ApiError::ResourceExhausted));
    }

    #[test]
    fn unable_to_extract_key_converts_to_internal() {
        assert!(matches!(
            ApiError::from(GovernorError::UnableToExtractKey),
            ApiError::Internal
        ));
    }

    #[test]
    fn too_many_requests_maps_to_429() {
        let response = error_response_handler(GovernorError::TooManyRequests {
            wait_time: 1,
            headers: None,
        });
        assert_eq!(response.status(), StatusCode::TOO_MANY_REQUESTS);
    }

    #[test]
    fn unable_to_extract_key_maps_to_500() {
        let response = error_response_handler(GovernorError::UnableToExtractKey);
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }

    #[test]
    fn other_error_maps_to_500() {
        let response = error_response_handler(GovernorError::Other {
            code: StatusCode::BAD_GATEWAY,
            msg: None,
            headers: None,
        });
        assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    }
}
