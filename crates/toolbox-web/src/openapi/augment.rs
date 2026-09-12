//! Amending an assembled spec with what every operation needs.

use utoipa::openapi::{ContentBuilder, OpenApi, RefOr, Response, ResponseBuilder};

/// The status codes every operation can produce, whether or not it says so.
const STANDARD_ERRORS: &[(&str, &str)] = &[
    ("400", "The request was malformed or failed validation"),
    ("401", "No credentials, or credentials that did not verify"),
    ("403", "Authenticated, but not permitted"),
    ("404", "No such resource"),
    ("409", "The request conflicts with current state"),
    ("429", "Rate limited; see Retry-After"),
    (
        "500",
        "An internal failure. The body carries a code and a request id, never a detail",
    ),
];

/// Attach the standard error responses to every operation.
///
/// Without this, each handler hand-annotates seven responses, which means in
/// practice that most annotate none and the spec claims endpoints cannot fail.
///
/// # Arguments
///
/// * `api` - The spec to amend in place. Every operation gains the responses it
///   can actually produce.
pub fn with_standard_errors(api: &mut OpenApi) {
    for item in api.paths.paths.values_mut() {
        // PathItem holds one Option per HTTP method rather than a map, so the
        // operations are enumerated rather than iterated.
        let operations = [
            item.get.as_mut(),
            item.put.as_mut(),
            item.post.as_mut(),
            item.delete.as_mut(),
            item.options.as_mut(),
            item.head.as_mut(),
            item.patch.as_mut(),
            item.trace.as_mut(),
        ];
        for operation in operations.into_iter().flatten() {
            for (status, description) in STANDARD_ERRORS {
                if operation.responses.responses.contains_key(*status) {
                    continue;
                }
                operation
                    .responses
                    .responses
                    .insert((*status).to_owned(), problem_response(description));
            }
        }
    }
}

/// One standard error response, referencing the shared problem schema rather
/// than repeating it.
///
/// # Arguments
///
/// * `description` - What this status means, as it appears in the docs page.
fn problem_response(description: &str) -> RefOr<Response> {
    RefOr::T(
        ResponseBuilder::new()
            .description(description)
            .content(toolbox_error::PROBLEM_JSON, ContentBuilder::new().build())
            .build(),
    )
}

/// Declare bearer-token security on the whole document.
///
/// # Arguments
///
/// * `api` - The spec to amend in place. Without this the docs page has no way
///   to send a token.
pub fn bearer_security(api: &mut OpenApi) {
    use utoipa::openapi::security::{HttpAuthScheme, HttpBuilder, SecurityScheme};

    let components = api.components.get_or_insert_with(Default::default);
    components.add_security_scheme(
        "bearer",
        SecurityScheme::Http(
            HttpBuilder::new()
                .scheme(HttpAuthScheme::Bearer)
                .bearer_format("JWT")
                .build(),
        ),
    );
}
