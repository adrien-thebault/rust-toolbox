//! The HTTP error type.
//!
//! It unifies every error that can reach a handler into one response shape, and
//! it removes two traps the obvious type had - serving `application/json` while
//! claiming RFC 7807, and putting raw database text in a 5xx body sent to
//! anonymous callers.

use axum::response::{IntoResponse, Response};
use http::{HeaderValue, StatusCode, header};
use toolbox_core::{ErrorInfo, ErrorKind, PROBLEM_JSON, Problem, ServiceError};
use toolbox_server::trace_context::current_request_id;
use tracing::error;

/// An error on its way to an HTTP response.
///
/// The `source` is private on purpose: it is logged, and it is never
/// serialized.
#[derive(Debug)]
pub struct ApiError {
    /// The HTTP status to send.
    status: StatusCode,
    /// Boxed because `Result<T, ApiError>` is the return type of every
    /// handler, so the error's size is paid on the success path too. Inline it
    /// and that is ~240 bytes per return; boxed it is ~48.
    problem: Box<Problem>,
    /// The underlying error, logged but never serialized.
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
    /// Seconds for a `Retry-After` header, when the status warrants one.
    retry_after: Option<u64>,
}

impl ApiError {
    /// An error with a status and a title.
    ///
    /// # Arguments
    ///
    /// * `status` - The HTTP status to answer with.
    /// * `title` - The short summary, the same for every occurrence of this
    ///   error type.
    pub fn new(status: StatusCode, title: impl Into<String>) -> Self {
        Self {
            status,
            problem: Box::new(Problem::new(status.as_u16(), title)),
            source: None,
            retry_after: None,
        }
    }

    /// The error for a kind, with the toolbox's status mapping applied.
    ///
    /// # Arguments
    ///
    /// * `kind` - The transport-neutral failure kind, mapped to a status by
    ///   [`status_for`].
    /// * `title` - The short summary.
    pub fn of_kind(kind: ErrorKind, title: impl Into<String>) -> Self {
        Self::new(status_for(kind), title)
    }

    /// Attach the underlying cause. Logged on 5xx, never serialized.
    ///
    /// # Arguments
    ///
    /// * `source` - The underlying cause. It is logged and never serialized,
    ///   which is what stops database text reaching an anonymous caller.
    #[must_use]
    pub fn with_source(mut self, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// Set the human-readable detail. Cleared before serialization on 5xx.
    ///
    /// # Arguments
    ///
    /// * `detail` - What went wrong in this specific case. Cleared before
    ///   serialization on a 5xx.
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.problem.detail = Some(detail.into());
        self
    }

    /// Set the machine-readable code clients branch on.
    ///
    /// # Arguments
    ///
    /// * `code` - The stable identifier a client branches on. It must survive a
    ///   reworded message.
    #[must_use]
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.problem.code = Some(code.into());
        self
    }

    /// Attach one metadata entry.
    ///
    /// # Arguments
    ///
    /// * `key` - The extension member name.
    /// * `value` - Its value. It is serialized, so it must carry nothing
    ///   internal.
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.problem.metadata.insert(key.into(), value.into());
        self
    }

    /// How many seconds to wait before retrying. Becomes a `Retry-After`
    /// header, which a naive limiter computed and then discarded.
    ///
    /// # Arguments
    ///
    /// * `seconds` - How long to wait. It becomes a `Retry-After` header, which
    ///   is the number a naive limiter computes and then discards.
    #[must_use]
    pub fn with_retry_after(mut self, seconds: u64) -> Self {
        self.retry_after = Some(seconds);
        self
    }

    /// The status this will respond with.
    #[must_use]
    pub fn status(&self) -> StatusCode {
        self.status
    }

    /// The problem document, before redaction.
    #[must_use]
    pub fn problem(&self) -> &Problem {
        &self.problem
    }

    /// A 404.
    ///
    /// # Arguments
    ///
    /// * `detail` - What was not found. It reaches the caller, so it must not
    ///   confirm the existence of anything they may not see.
    pub fn not_found(detail: impl Into<String>) -> Self {
        Self::of_kind(ErrorKind::NotFound, "Not Found").with_detail(detail)
    }

    /// A 400.
    ///
    /// # Arguments
    ///
    /// * `detail` - What was wrong with the request.
    pub fn bad_request(detail: impl Into<String>) -> Self {
        Self::of_kind(ErrorKind::InvalidArgument, "Invalid Argument").with_detail(detail)
    }

    /// A 401.
    pub fn unauthenticated() -> Self {
        Self::of_kind(ErrorKind::Unauthenticated, "Unauthenticated")
    }

    /// A 403.
    ///
    /// # Arguments
    ///
    /// * `detail` - Why the caller may not do this. Distinct from a 401, which
    ///   means they are not authenticated at all.
    pub fn forbidden(detail: impl Into<String>) -> Self {
        Self::of_kind(ErrorKind::PermissionDenied, "Permission Denied").with_detail(detail)
    }

    /// A 500 whose detail never reaches the caller.
    ///
    /// # Arguments
    ///
    /// * `source` - The cause. It is logged in full and never serialized, which
    ///   is the entire difference between this and the other constructors.
    pub fn internal(source: impl std::error::Error + Send + Sync + 'static) -> Self {
        Self::of_kind(ErrorKind::Internal, "Internal Server Error").with_source(source)
    }
}

impl std::fmt::Display for ApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{} {}", self.status.as_u16(), self.problem.title)
    }
}

impl std::error::Error for ApiError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| &**e as &(dyn std::error::Error + 'static))
    }
}

/// The one mapping from a transport-neutral kind to an HTTP status.
///
/// Two are worth knowing: `Conflict` is 409 rather than 422, because
/// optimistic locking and duplicate keys are what produce it; and
/// `FailedPrecondition` is 412 rather than 400, because the request was
/// well-formed and the *state* was wrong - which is what a client needs in
/// order to know whether retrying could ever help.
///
/// # Arguments
///
/// * `kind` - The transport-neutral failure kind to translate.
#[must_use]
pub fn status_for(kind: ErrorKind) -> StatusCode {
    match kind {
        ErrorKind::NotFound => StatusCode::NOT_FOUND,
        ErrorKind::InvalidArgument => StatusCode::BAD_REQUEST,
        ErrorKind::Unauthenticated => StatusCode::UNAUTHORIZED,
        ErrorKind::PermissionDenied => StatusCode::FORBIDDEN,
        ErrorKind::Conflict => StatusCode::CONFLICT,
        ErrorKind::ResourceExhausted => StatusCode::TOO_MANY_REQUESTS,
        ErrorKind::FailedPrecondition => StatusCode::PRECONDITION_FAILED,
        ErrorKind::Timeout => StatusCode::GATEWAY_TIMEOUT,
        ErrorKind::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
        ErrorKind::Unimplemented => StatusCode::NOT_IMPLEMENTED,
        // ErrorKind is #[non_exhaustive], so a kind added upstream lands on the
        // same arm as Internal rather than breaking the build. A 500 is the
        // safe default: it is the only status whose body is redacted.
        ErrorKind::Internal | _ => StatusCode::INTERNAL_SERVER_ERROR,
    }
}

/// Any domain error becomes an `ApiError`.
///
/// Legal because `ApiError` is local - the orphan-rule problem only ever
/// applied to `tonic::Status`, which is foreign. See `toolbox-grpc`.
impl<E: ServiceError + Send + Sync + 'static> From<E> for ApiError {
    fn from(err: E) -> Self {
        let status = status_for(err.kind());
        let problem = Box::new(Problem::from_service_error(&err, status.as_u16()));
        Self {
            status,
            problem,
            source: Some(Box::new(err)),
            retry_after: None,
        }
    }
}

/// A gRPC error that crossed into HTTP.
///
/// This is the seam that lets `toolbox-web` stay free of tonic: `toolbox-grpc`
/// turns a `Status` into an `ErrorInfo`, and this turns that into a response.
/// Neither crate needs the other.
impl ApiError {
    /// Build from an `ErrorInfo` and the kind the caller decoded alongside it.
    ///
    /// # Arguments
    ///
    /// * `info` - The machine-readable part the backend sent, replayed to the
    ///   client unchanged.
    /// * `kind` - The failure kind the caller decoded from the status, which is
    ///   what picks the HTTP status here.
    #[must_use]
    pub fn from_error_info(info: ErrorInfo, kind: ErrorKind) -> Self {
        let status = status_for(kind);
        let mut problem = Problem::new(
            status.as_u16(),
            status.canonical_reason().unwrap_or("Error"),
        );
        problem.code = Some(info.code);
        problem.domain = Some(info.domain);
        problem.metadata = info.metadata;
        Self {
            status,
            problem: Box::new(problem),
            source: None,
            retry_after: None,
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(mut self) -> Response {
        if self.status.is_server_error() {
            // The detail and metadata of an internal failure are built from the
            // error's own Display: database text, connection strings, host
            // names. Log them, never send them.
            error!(
                status = self.status.as_u16(),
                code = self.problem.code.as_deref().unwrap_or("-"),
                error = self
                    .source
                    .as_ref()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                detail = self.problem.detail.as_deref().unwrap_or("-"),
                "request failed"
            );
            self.problem.redact();
        }

        self.problem.request_id = current_request_id();
        self.problem.status = self.status.as_u16();

        let body = serde_json::to_vec(&self.problem).unwrap_or_else(|_| {
            br#"{"type":"about:blank","title":"Internal Server Error","status":500}"#.to_vec()
        });

        let mut response = Response::builder()
            .status(self.status)
            .header(header::CONTENT_TYPE, HeaderValue::from_static(PROBLEM_JSON))
            .body(axum::body::Body::from(body))
            .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response());

        if let Some(seconds) = self.retry_after
            && let Ok(value) = HeaderValue::from_str(&seconds.to_string())
        {
            response.headers_mut().insert(header::RETRY_AFTER, value);
        }
        response
    }
}
