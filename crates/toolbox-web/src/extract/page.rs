//! The pagination extractor.

use axum::extract::{FromRequestParts, Query};
use http::request::Parts;
use serde::Deserialize;
use toolbox_core::{MAX_LIMIT, PageRequest, Sort};

use crate::error::ApiError;

/// The raw query parameters, before validation.
#[derive(Debug, Deserialize)]
struct RawPage {
    /// `?offset=`.
    offset: Option<i64>,
    /// `?limit=`.
    limit: Option<i64>,
    /// `?sort=`.
    sort: Option<String>,
}

/// `?offset=&limit=&sort=` turned into a validated [`PageRequest`].
///
/// Omitting both `offset` and `limit` gives an unpaged request, so a lookup
/// table needs no parameters. Supplying either gives a bounded one, with the
/// other defaulted.
#[derive(Debug, Clone)]
pub struct PageQuery(pub PageRequest);

impl PageQuery {
    /// The request.
    #[must_use]
    pub fn request(&self) -> &PageRequest {
        &self.0
    }

    /// Take the request.
    #[must_use]
    pub fn into_request(self) -> PageRequest {
        self.0
    }
}

impl<S: Send + Sync> FromRequestParts<S> for PageQuery {
    type Rejection = ApiError;

    async fn from_request_parts(parts: &mut Parts, state: &S) -> Result<Self, Self::Rejection> {
        let Query(raw) = Query::<RawPage>::from_request_parts(parts, state)
            .await
            .map_err(|e| ApiError::bad_request(e.body_text()).with_code("MALFORMED_QUERY"))?;

        let sort = raw
            .sort
            .as_deref()
            .map(Sort::parse)
            .transpose()
            .map_err(|e| ApiError::bad_request(e.to_string()).with_code("INVALID_SORT"))?
            .unwrap_or_default();

        match (raw.offset, raw.limit) {
            (None, None) => Ok(Self(PageRequest::unpaged(sort))),
            (offset, limit) => {
                let request = PageRequest::paged(offset.unwrap_or(0), limit.unwrap_or(50), sort)
                    .map_err(|e| {
                        ApiError::bad_request(e.to_string())
                            .with_code("INVALID_PAGE")
                            .with_metadata("max_limit", MAX_LIMIT.to_string())
                    })?;
                Ok(Self(request))
            }
        }
    }
}
