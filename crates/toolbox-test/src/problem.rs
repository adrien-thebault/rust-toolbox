//! Asserting on an RFC 9457 response.

/// What a problem response actually said.
#[derive(Debug, Clone)]
pub struct ProblemResponse {
    /// The HTTP status.
    pub status: u16,
    /// The `Content-Type` header.
    pub content_type: String,
    /// The parsed body.
    pub body: serde_json::Value,
}

impl ProblemResponse {
    /// Build from a status, a content type and a body.
    ///
    /// # Arguments
    ///
    /// * `status` - The HTTP status the response carried.
    /// * `content_type` - The response's content type, kept so an assertion can
    ///   check it is `application/problem+json`.
    /// * `body` - The raw body, parsed as JSON.
    ///
    /// # Panics
    /// When the body is not JSON, which for a problem response is itself the
    /// failure worth reporting.
    #[must_use]
    pub fn new(status: u16, content_type: &str, body: &str) -> Self {
        Self {
            status,
            content_type: content_type.to_owned(),
            body: serde_json::from_str(body)
                .unwrap_or_else(|e| panic!("response body is not JSON ({e}): {body}")),
        }
    }

    /// The `code` extension member.
    #[must_use]
    pub fn code(&self) -> Option<&str> {
        self.body.get("code").and_then(serde_json::Value::as_str)
    }

    /// One `metadata` entry.
    ///
    /// # Arguments
    ///
    /// * `key` - The extension member to read, for asserting on the detail a
    ///   handler attached.
    #[must_use]
    pub fn metadata(&self, key: &str) -> Option<&str> {
        self.body.get("metadata")?.get(key)?.as_str()
    }
}

/// Assert a response is an RFC 9457 problem with a given status and code.
///
/// Checks the media type too, because serving `application/json` while
/// claiming RFC 9457 is the exact bug this whole shape exists to prevent.
///
/// ```ignore
/// assert_problem!(response, 404, "EVENT_NOT_FOUND");
/// assert_problem!(response, 400, "VALIDATION_FAILED", "email");
/// ```
#[macro_export]
macro_rules! assert_problem {
    ($response:expr, $status:expr, $code:expr) => {{
        let problem: &$crate::problem::ProblemResponse = &$response;
        assert_eq!(problem.status, $status, "status; body was {}", problem.body);
        assert_eq!(
            problem.content_type, "application/problem+json",
            "an error must be problem+json, not plain json"
        );
        assert_eq!(
            problem.code(),
            Some($code),
            "code; body was {}",
            problem.body
        );
    }};
    ($response:expr, $status:expr, $code:expr, $field:expr) => {{
        $crate::assert_problem!($response, $status, $code);
        let problem: &$crate::problem::ProblemResponse = &$response;
        assert!(
            problem.metadata($field).is_some(),
            "expected metadata for `{}`; body was {}",
            $field,
            problem.body
        );
    }};
}
