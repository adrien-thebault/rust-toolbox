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

    CorsLayer::new()
        .allow_origin(AllowOrigin::list(parsed))
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

/// As [`cors`], plus the localhost origins a development frontend uses.
///
/// Never call this in production: it permits any page served from localhost to
/// make credentialed requests, which is fine on a laptop and a hole anywhere
/// else. The name says `dev` so it is greppable before a release.
///
/// # Arguments
///
/// * `origins` - The production origins to allow, on top of the localhost ones
///   this adds.
pub fn dev_and(origins: &[String]) -> CorsLayer {
    let mut all = origins.to_vec();
    all.extend([
        "http://localhost:5173".to_owned(),
        "http://localhost:3000".to_owned(),
        "http://127.0.0.1:5173".to_owned(),
    ]);
    cors(&all)
}
