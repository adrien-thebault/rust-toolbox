//! CORS, as one function rather than a module.

use http::{HeaderValue, Method, header};
use tower_http::cors::{AllowOrigin, CorsLayer};

/// A CORS layer permitting the given origins, with credentials.
///
/// Credentials are on because a browser client authenticated by cookie needs
/// them, and a wildcard origin is impossible with credentials anyway - so the
/// origin list is required rather than defaulted.
///
/// # Arguments
///
/// * `origins` - The exact origins allowed. A wildcard is impossible here
///   anyway, because the layer sends credentials.
pub fn cors(origins: &[String]) -> CorsLayer {
    let parsed: Vec<HeaderValue> = origins
        .iter()
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect();

    base().allow_origin(AllowOrigin::list(parsed))
}

/// As [`cors`], but also reflecting any `http(s)://localhost` or loopback
/// origin, on any port.
///
/// Never in production: it lets any page served from the loopback interface
/// make credentialed requests, which is fine on a laptop and a hole anywhere
/// else. The `localhost` in the name is what makes it greppable before a
/// release. A literal wildcard is not an option - credentials forbid it - so a
/// dev frontend on an arbitrary Vite/CRA port is matched by host instead.
///
/// # Arguments
///
/// * `origins` - The production origins to allow, on top of the loopback ones
///   this reflects.
pub fn cors_localhost(origins: &[String]) -> CorsLayer {
    let allowed: Vec<HeaderValue> = origins
        .iter()
        .filter_map(|o| HeaderValue::from_str(o).ok())
        .collect();

    base().allow_origin(AllowOrigin::predicate(move |origin, _parts| {
        if allowed.contains(origin) {
            return true;
        }
        // Any `http(s)://localhost` / loopback origin, on any port.
        let Ok(text) = origin.to_str() else {
            return false;
        };
        let authority = text
            .strip_prefix("http://")
            .or_else(|| text.strip_prefix("https://"))
            .unwrap_or(text)
            .split('/')
            .next()
            .unwrap_or(text);
        let host = authority.strip_prefix('[').map_or_else(
            || authority.split(':').next().unwrap_or(authority),
            |v6| v6.split(']').next().unwrap_or(v6),
        );
        matches!(host, "localhost" | "127.0.0.1" | "::1")
    }))
}

/// The parts of the layer both constructors share.
fn base() -> CorsLayer {
    CorsLayer::new()
        .allow_methods([
            Method::GET,
            Method::POST,
            Method::PUT,
            Method::PATCH,
            Method::DELETE,
            Method::OPTIONS,
        ])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE, header::ACCEPT])
        .allow_credentials(true)
}
