//! Validating extractors.
//!
//! It bridges `garde` and RFC 9457, which do not know about each other.
//! `garde::Report` iterates as flat `(path, error)` pairs, which is
//! `Problem.metadata` already for why that decided the crate.

use axum::extract::{FromRequest, Request, rejection::JsonRejection};
use garde::Validate;
use http::StatusCode;

use crate::error::ApiError;

/// The code a client branches on when a request failed validation.
pub const VALIDATION_FAILED: &str = "VALIDATION_FAILED";

/// A JSON body that parsed *and* validated.
#[derive(Debug, Clone, Copy, Default)]
pub struct ValidJson<T>(pub T);

impl<T, S> FromRequest<S> for ValidJson<T>
where
    T: serde::de::DeserializeOwned + Validate<Context = ()> + 'static,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request(req: Request, state: &S) -> Result<Self, Self::Rejection> {
        let axum::Json(value) = axum::Json::<T>::from_request(req, state)
            .await
            .map_err(|e| malformed(&e))?;
        validate(&value)?;
        Ok(Self(value))
    }
}

/// A query string that parsed *and* validated.
#[derive(Debug, Clone, Copy, Default)]
pub struct ValidQuery<T>(pub T);

impl<T, S> axum::extract::FromRequestParts<S> for ValidQuery<T>
where
    T: serde::de::DeserializeOwned + Validate<Context = ()> + 'static,
    S: Send + Sync,
{
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut http::request::Parts,
        state: &S,
    ) -> Result<Self, Self::Rejection> {
        let axum::extract::Query(value) =
            axum::extract::Query::<T>::from_request_parts(parts, state)
                .await
                .map_err(|e| ApiError::bad_request(e.body_text()).with_code("MALFORMED_QUERY"))?;
        validate(&value)?;
        Ok(Self(value))
    }
}

/// Turn a `garde` report into a problem whose metadata names every bad field.
///
/// # Arguments
///
/// * `value` - The parsed body or query. Every failing field is named in the
///   problem's metadata, so a client can highlight them.
fn validate<T: Validate<Context = ()>>(value: &T) -> Result<(), ApiError> {
    let Err(report) = value.validate() else {
        return Ok(());
    };

    let mut err = ApiError::new(StatusCode::BAD_REQUEST, "Invalid Argument")
        .with_code(VALIDATION_FAILED)
        .with_detail("one or more fields are invalid");

    for (path, error) in report.iter() {
        // `path` renders as `address.postcode` for a nested field, which is
        // the spelling a frontend needs to highlight the right input.
        err = err.with_metadata(path.to_string(), error.to_string());
    }
    Err(err)
}

/// A body that did not parse is a client mistake, and its message names the
/// offending field without disclosing anything of ours.
///
/// # Arguments
///
/// * `rejection` - What axum's JSON extractor refused. Its message names the
///   offending field without disclosing anything of ours.
fn malformed(rejection: &JsonRejection) -> ApiError {
    ApiError::bad_request(rejection.body_text()).with_code("MALFORMED_BODY")
}
