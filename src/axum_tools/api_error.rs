//! a REST-facing error type any gateway handler can return directly, with a
//! `tonic::Status` conversion so proxying a gRPC error is a one-liner
//! (`.map_err(ApiError::from)?`).
//!
//! Every response body is `application/problem+json` carrying
//! `code`/`domain`/`metadata` (see [`Problem`]). A [`Status`] built from a
//! [`ServiceError`](crate::tonic_tools::ServiceError) passes its
//! code/domain/metadata straight through as [`ApiError::Service`]; anything
//! else gets a code mechanically derived from its `ApiError` variant.

use crate::axum_tools::auth::{AuthError, JwtError};
use axum::{
    Json,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde_derive::Serialize;
use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
};
use thiserror::Error;
use tonic::{Code, Status};

/// domain for every synthesized (non-[`ServiceError`](crate::tonic_tools::ServiceError))
/// `ApiError`, mirroring `google.rpc.ErrorInfo.domain`.
const GATEWAY_DOMAIN: &str = "gateway";

/// an error that maps directly onto an `application/problem+json`
/// ([RFC 7807](https://www.rfc-editor.org/rfc/rfc7807)) HTTP response with
/// `code`/`domain`/`metadata` (see the module doc comment). Every variant
/// but [`Self::Service`] is a fixed, parameterless HTTP error.
#[derive(Debug, Error)]
pub enum ApiError {
    /// maps to `404 Not Found`
    #[error("resource not found")]
    NotFound,
    /// maps to `400 Bad Request`
    #[error("invalid argument")]
    InvalidArgument,
    /// maps to `401 Unauthorized`
    #[error("authentication required")]
    Unauthenticated,
    /// maps to `403 Forbidden`
    #[error("insufficient permissions")]
    Forbidden,
    /// maps to `409 Conflict`
    #[error("conflict")]
    Conflict,
    /// maps to `429 Too Many Requests`
    #[error("resource exhausted")]
    ResourceExhausted,
    /// maps to `500 Internal Server Error`
    #[error("internal server error")]
    Internal,
    /// maps to `501 Not Implemented`
    #[error("not implemented")]
    Unimplemented,
    /// maps to `503 Service Unavailable`
    #[error("service unavailable")]
    Unavailable,
    /// maps to `504 Gateway Timeout`
    #[error("upstream timeout")]
    Timeout,
    /// a decoded [`ServiceError`](crate::tonic_tools::ServiceError), passed
    /// through with its own status/code/domain/metadata. Boxed so this, the
    /// largest variant, doesn't inflate every `ApiError` by its size.
    #[error("{0}")]
    Service(Box<ServiceErrorInfo>),
}

/// the fields carried by [`ApiError::Service`]; see that variant's doc
/// comment for why it's boxed rather than inline.
#[derive(Debug)]
pub struct ServiceErrorInfo {
    status: StatusCode,
    code: String,
    domain: String,
    message: String,
    metadata: HashMap<String, String>,
}

impl Display for ServiceErrorInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// the `application/problem+json` response body.
#[derive(Serialize)]
struct Problem {
    code: String,
    domain: String,
    metadata: HashMap<String, String>,
}

impl ApiError {
    fn status(&self) -> StatusCode {
        match self {
            Self::NotFound => StatusCode::NOT_FOUND,
            Self::InvalidArgument => StatusCode::BAD_REQUEST,
            Self::Unauthenticated => StatusCode::UNAUTHORIZED,
            Self::Forbidden => StatusCode::FORBIDDEN,
            Self::Conflict => StatusCode::CONFLICT,
            Self::ResourceExhausted => StatusCode::TOO_MANY_REQUESTS,
            Self::Internal => StatusCode::INTERNAL_SERVER_ERROR,
            Self::Unimplemented => StatusCode::NOT_IMPLEMENTED,
            Self::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
            Self::Timeout => StatusCode::GATEWAY_TIMEOUT,
            Self::Service(info) => info.status,
        }
    }

    /// this error's wire code: derived from the variant name for every
    /// synthesized case, or passed through unchanged for [`Self::Service`].
    fn code(&self) -> &str {
        match self {
            Self::NotFound => "NOT_FOUND",
            Self::InvalidArgument => "INVALID_ARGUMENT",
            Self::Unauthenticated => "UNAUTHENTICATED",
            Self::Forbidden => "FORBIDDEN",
            Self::Conflict => "CONFLICT",
            Self::ResourceExhausted => "RESOURCE_EXHAUSTED",
            Self::Internal => "INTERNAL",
            Self::Unimplemented => "UNIMPLEMENTED",
            Self::Unavailable => "UNAVAILABLE",
            Self::Timeout => "TIMEOUT",
            Self::Service(info) => &info.code,
        }
    }

    fn domain(&self) -> &str {
        match self {
            Self::Service(info) => &info.domain,
            _ => GATEWAY_DOMAIN,
        }
    }

    /// structured parameters for [`code`](Self::code), plus this error's
    /// `Display` message under `"detail"` - unless the service already
    /// provided its own `"detail"` key, which wins.
    fn metadata(&self) -> HashMap<String, String> {
        let mut metadata = match self {
            Self::Service(info) => info.metadata.clone(),
            _ => HashMap::new(),
        };
        metadata
            .entry("detail".to_string())
            .or_insert_with(|| self.to_string());
        metadata
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let status = self.status();
        let body = Problem {
            code: self.code().to_string(),
            domain: self.domain().to_string(),
            metadata: self.metadata(),
        };
        (status, Json(body)).into_response()
    }
}

/// the canonical gRPC -> HTTP mapping (following
/// <https://github.com/googleapis/googleapis/blob/master/google/rpc/code.proto>),
/// as the flat `ApiError` variant each `Code` collapses into.
impl From<Code> for ApiError {
    fn from(code: Code) -> Self {
        match code {
            Code::NotFound => Self::NotFound,
            Code::InvalidArgument | Code::OutOfRange => Self::InvalidArgument,
            Code::Unauthenticated => Self::Unauthenticated,
            Code::PermissionDenied => Self::Forbidden,
            Code::FailedPrecondition | Code::AlreadyExists | Code::Aborted => Self::Conflict,
            Code::ResourceExhausted => Self::ResourceExhausted,
            Code::Unimplemented => Self::Unimplemented,
            Code::Unavailable => Self::Unavailable,
            Code::DeadlineExceeded => Self::Timeout,
            _ => Self::Internal,
        }
    }
}

impl From<Status> for ApiError {
    fn from(status: Status) -> Self {
        // the flat variant this status collapses into - used directly when
        // the status carries no ErrorInfo, and for its HTTP status otherwise
        let fallback = Self::from(status.code());

        match crate::tonic_tools::DecodedServiceError::from_status(&status) {
            Some(decoded) => Self::Service(Box::new(ServiceErrorInfo {
                status: fallback.status(),
                code: decoded.code,
                domain: decoded.domain,
                message: status.message().to_string(),
                metadata: decoded.metadata,
            })),
            None => fallback,
        }
    }
}

impl From<AuthError> for ApiError {
    fn from(err: AuthError) -> Self {
        match err {
            AuthError::InvalidCredentials | AuthError::UnsupportedCredential => {
                Self::Unauthenticated
            }
            AuthError::InsufficientRole => Self::Forbidden,
        }
    }
}

impl From<JwtError> for ApiError {
    fn from(err: JwtError) -> Self {
        match err {
            // signing failed on our end (e.g. a malformed key) - not the
            // caller's fault, so this isn't an auth failure
            JwtError::Encode(_) => Self::Internal,
            // the presented token itself doesn't check out
            JwtError::Decode(_) => Self::Unauthenticated,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tonic_tools::ServiceError;
    use std::fmt;

    #[derive(Debug)]
    struct TestError;

    impl fmt::Display for TestError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            write!(f, "widget is referenced elsewhere")
        }
    }

    impl std::error::Error for TestError {}

    impl ServiceError for TestError {
        fn code(&self) -> &'static str {
            "WIDGET_IN_USE"
        }

        fn domain(&self) -> &'static str {
            "example-service"
        }

        fn status_code(&self) -> Code {
            Code::FailedPrecondition
        }

        fn metadata(&self) -> HashMap<String, String> {
            HashMap::from([("references".to_string(), "3".to_string())])
        }
    }

    #[test]
    fn service_error_round_trips_through_status_into_api_error() {
        let status = TestError.to_status();
        let api_error: ApiError = status.into();

        match api_error {
            ApiError::Service(info) => {
                assert_eq!(info.status, StatusCode::CONFLICT);
                assert_eq!(info.code, "WIDGET_IN_USE");
                assert_eq!(info.domain, "example-service");
                assert_eq!(info.metadata.get("references"), Some(&"3".to_string()));
            }
            other => panic!("expected ApiError::Service, got {other:?}"),
        }
    }

    #[test]
    fn plain_status_without_error_info_falls_back_to_flat_variant() {
        let status = Status::not_found("widget 42");
        let api_error: ApiError = status.into();
        assert!(matches!(api_error, ApiError::NotFound));
    }

    #[test]
    fn grpc_codes_map_to_the_canonical_http_statuses() {
        let cases = [
            (Code::NotFound, StatusCode::NOT_FOUND),
            (Code::InvalidArgument, StatusCode::BAD_REQUEST),
            (Code::OutOfRange, StatusCode::BAD_REQUEST),
            (Code::Unauthenticated, StatusCode::UNAUTHORIZED),
            (Code::PermissionDenied, StatusCode::FORBIDDEN),
            (Code::FailedPrecondition, StatusCode::CONFLICT),
            (Code::AlreadyExists, StatusCode::CONFLICT),
            (Code::Aborted, StatusCode::CONFLICT),
            (Code::ResourceExhausted, StatusCode::TOO_MANY_REQUESTS),
            (Code::Unimplemented, StatusCode::NOT_IMPLEMENTED),
            (Code::Unavailable, StatusCode::SERVICE_UNAVAILABLE),
            (Code::DeadlineExceeded, StatusCode::GATEWAY_TIMEOUT),
            (Code::Internal, StatusCode::INTERNAL_SERVER_ERROR),
            (Code::Unknown, StatusCode::INTERNAL_SERVER_ERROR),
        ];
        for (code, expected) in cases {
            assert_eq!(ApiError::from(code).status(), expected, "for {code:?}");
        }
    }

    #[test]
    fn service_provided_detail_metadata_is_not_clobbered() {
        #[derive(Debug)]
        struct DetailError;

        impl fmt::Display for DetailError {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "display message")
            }
        }

        impl std::error::Error for DetailError {}

        impl ServiceError for DetailError {
            fn code(&self) -> &'static str {
                "DETAILED"
            }

            fn domain(&self) -> &'static str {
                "example-service"
            }

            fn status_code(&self) -> Code {
                Code::InvalidArgument
            }

            fn metadata(&self) -> HashMap<String, String> {
                HashMap::from([("detail".to_string(), "service detail".to_string())])
            }
        }

        let api_error = ApiError::from(DetailError.to_status());
        assert_eq!(
            api_error.metadata().get("detail"),
            Some(&"service detail".to_string())
        );
    }
}
