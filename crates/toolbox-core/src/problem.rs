//! RFC 9457 problem details.
//!
//! RFC 9457 already defines an error body for HTTP, so the toolbox implements
//! it rather than inventing a shape. The obvious code claimed RFC 7807 in five
//! documents while serving `application/json` with a bespoke body.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{ErrorKind, ServiceError};

/// The media type RFC 9457 requires. Serving `application/json` instead is the
/// single most common way to get this wrong.
pub const PROBLEM_JSON: &str = "application/problem+json";

/// The default `type` when a problem has no dereferenceable documentation URI.
pub const ABOUT_BLANK: &str = "about:blank";

/// An RFC 9457 problem document.
///
/// The registered members are `type`, `title`, `status`, `detail` and
/// `instance`; `code`, `domain`, `metadata` and `request_id` are extension
/// members, which RFC 9457 explicitly permits.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Problem {
    /// A URI identifying the problem type. Defaults to `about:blank`.
    #[serde(rename = "type")]
    pub type_: String,
    /// A short, human-readable summary. Stable for a given `type`.
    pub title: String,
    /// The HTTP status code, repeated here so the body is self-contained.
    pub status: u16,
    /// A human-readable explanation specific to this occurrence.
    ///
    /// Cleared before serialization on 5xx: it is the field that leaks
    /// database text to anonymous callers.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub detail: Option<String>,
    /// A URI identifying this specific occurrence.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub instance: Option<String>,
    /// The stable `ServiceError::code`, for clients that branch on the failure.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub code: Option<String>,
    /// The service that produced the error.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub domain: Option<String>,
    /// Structured context. Doubles as translation parameters for a frontend
    /// that uses `code` as the message key.
    #[serde(skip_serializing_if = "BTreeMap::is_empty", default)]
    pub metadata: BTreeMap<String, String>,
    /// The request id, so a user can quote it in a bug report.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub request_id: Option<String>,
}

impl Problem {
    /// A problem with only the two members RFC 9457 always wants.
    ///
    /// # Arguments
    ///
    /// * `status` - The HTTP status this problem will be sent with. It is
    ///   duplicated into the body because RFC 9457 requires it there too.
    /// * `title` - A short, human-readable summary that stays the same for
    ///   every occurrence of this problem type.
    pub fn new(status: u16, title: impl Into<String>) -> Self {
        Self {
            type_: ABOUT_BLANK.to_owned(),
            title: title.into(),
            status,
            detail: None,
            instance: None,
            code: None,
            domain: None,
            metadata: BTreeMap::new(),
            request_id: None,
        }
    }

    /// Build a problem from a domain error, taking its code, domain and
    /// metadata and using its `Display` as `detail`.
    ///
    /// # Arguments
    ///
    /// * `err` - The domain error. Its `ErrorInfo` becomes the `code`, `domain`
    ///   and `metadata` extensions.
    /// * `status` - The HTTP status to report, chosen by the caller because
    ///   this crate does not know about HTTP.
    pub fn from_service_error<E: ServiceError + ?Sized>(err: &E, status: u16) -> Self {
        Self {
            type_: ABOUT_BLANK.to_owned(),
            title: title_for(err.kind()).to_owned(),
            status,
            detail: Some(err.to_string()),
            instance: None,
            code: Some(err.code().to_owned()),
            domain: Some(err.domain().to_owned()),
            metadata: err.metadata(),
            request_id: None,
        }
    }

    /// Set the problem type URI.
    ///
    /// # Arguments
    ///
    /// * `type_` - A URI that documents this problem type. Defaults to
    ///   `about:blank` when there is nothing to dereference.
    #[must_use]
    pub fn with_type(mut self, type_: impl Into<String>) -> Self {
        self.type_ = type_.into();
        self
    }

    /// Set the human-readable detail.
    ///
    /// # Arguments
    ///
    /// * `detail` - An explanation specific to this occurrence, as opposed to
    ///   `title`, which describes the type.
    #[must_use]
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = Some(detail.into());
        self
    }

    /// Set the request id.
    ///
    /// # Arguments
    ///
    /// * `id` - The request id to echo, so a user quoting it from an error page
    ///   lands on the matching log lines.
    #[must_use]
    pub fn with_request_id(mut self, id: impl Into<String>) -> Self {
        self.request_id = Some(id.into());
        self
    }

    /// Attach one metadata entry.
    ///
    /// # Arguments
    ///
    /// * `key` - The extension member name, added alongside `code` and `domain`
    ///   rather than nested.
    /// * `value` - Its value. It is serialized to the client, so it must not
    ///   carry anything internal.
    #[must_use]
    pub fn with_metadata(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.metadata.insert(key.into(), value.into());
        self
    }

    /// Drop everything a 5xx response must not disclose.
    ///
    /// `detail` and `metadata` are built from the error's own `Display` and
    /// context, which for an internal failure is database or dependency text.
    pub fn redact(&mut self) {
        self.detail = None;
        self.metadata.clear();
    }
}

/// The stable title for a kind, so the same failure always reads the same way.
///
/// # Arguments
///
/// * `kind` - The transport-neutral failure kind to name.
#[must_use]
pub fn title_for(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::NotFound => "Not Found",
        ErrorKind::InvalidArgument => "Invalid Argument",
        ErrorKind::Unauthenticated => "Unauthenticated",
        ErrorKind::PermissionDenied => "Permission Denied",
        ErrorKind::Conflict => "Conflict",
        ErrorKind::ResourceExhausted => "Too Many Requests",
        ErrorKind::FailedPrecondition => "Precondition Failed",
        ErrorKind::Unimplemented => "Not Implemented",
        ErrorKind::Unavailable => "Service Unavailable",
        ErrorKind::Timeout => "Gateway Timeout",
        ErrorKind::Internal => "Internal Server Error",
    }
}
