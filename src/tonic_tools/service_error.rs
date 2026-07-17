use std::collections::HashMap;
use tonic::{Code, Status};
use tonic_types::{ErrorDetails, StatusExt};

/// implemented by a service's own domain error enum so it can flow from
/// gRPC all the way to the HTTP gateway carrying a structured
/// `google.rpc.ErrorInfo` (code/domain/metadata), instead of collapsing into
/// a bare status code and string message. See [`to_status`](Self::to_status)
/// for building the actual [`Status`], and `axum_tools::api_error` for the
/// decode side.
pub trait ServiceError: std::error::Error {
    /// this error's wire code, in SCREAMING_SNAKE_CASE (e.g. `"WIDGET_IN_USE"`)
    fn code(&self) -> &'static str;

    /// namespaces `code` so two services picking the same code by
    /// coincidence can't collide (mirrors `google.rpc.ErrorInfo`'s own
    /// `domain` field). Fixed per implementing type, e.g. `"my-service"`.
    fn domain(&self) -> &'static str;

    /// the gRPC status code this particular error maps onto
    fn status_code(&self) -> Code;

    /// structured parameters a caller can interpolate into a `code`'s
    /// message template, or otherwise act on - e.g. `{"count": "3"}`.
    /// Empty by default.
    fn metadata(&self) -> HashMap<String, String> {
        HashMap::new()
    }

    /// builds the [`Status`] for this error, attaching its
    /// code/domain/metadata as a `google.rpc.ErrorInfo`. Can't be a blanket
    /// `impl<E: ServiceError> From<E> for Status` (neither type is local to
    /// this crate, so a generic impl would violate Rust's orphan rules) -
    /// each implementing crate instead defines its own local `impl
    /// From<XError> for Status` whose body just calls this method.
    fn to_status(&self) -> Status {
        let mut details = ErrorDetails::new();
        details.set_error_info(self.code(), self.domain(), self.metadata());
        Status::with_error_details(self.status_code(), self.to_string(), details)
    }
}

/// the `google.rpc.ErrorInfo` fields decoded back out of a [`Status`], if it
/// carries one - i.e. if it was built by
/// [`ServiceError::to_status`] rather than being a raw infrastructure
/// failure with no structured detail attached.
#[derive(Debug, Clone)]
pub struct DecodedServiceError {
    /// [`ServiceError::code`]
    pub code: String,
    /// [`ServiceError::domain`]
    pub domain: String,
    /// [`ServiceError::metadata`]
    pub metadata: HashMap<String, String>,
}

impl DecodedServiceError {
    /// decodes the `google.rpc.ErrorInfo` out of `status`. `None` isn't a
    /// decoding failure, just a `Status` that wasn't built via
    /// [`ServiceError::to_status`] (e.g. a raw infrastructure failure).
    pub fn from_status(status: &Status) -> Option<Self> {
        let info = status.get_error_details().error_info()?.clone();
        Some(Self {
            code: info.reason,
            domain: info.domain,
            metadata: info.metadata,
        })
    }
}
