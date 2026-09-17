//! Pagination and sorting, in one representation that serves query strings,
//! protobuf messages and SQL alike.
//!
//! It encodes the decisions - bounds validated at construction, overflow
//! saturating, one sort representation - that were previously re-made per call
//! site, usually as a silent clamp.

use serde::{Deserialize, Deserializer, Serialize, Serializer, de::Error as _};

use crate::sort::Sort;

/// The largest page a caller may request, unless a call site overrides it.
///
/// A cap has to exist somewhere: without one, `limit=100000000` is a denial of
/// service against your own database.
pub const MAX_LIMIT: i64 = 100_000;

/// Why a `PageRequest` could not be built.
///
/// Construction fails rather than clamping: a silently truncated page is a
/// caller that thinks it has all the data and does not.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PageError {
    /// The offset was negative.
    #[error("offset must not be negative, got {0}")]
    NegativeOffset(i64),
    /// The limit was zero or negative.
    #[error("limit must be positive, got {0}")]
    NonPositiveLimit(i64),
    /// The limit exceeded the maximum this endpoint permits.
    #[error("limit {requested} exceeds the maximum of {max}")]
    LimitTooLarge {
        /// What the caller asked for.
        requested: i64,
        /// The cap in force.
        max: i64,
    },
    /// A sort direction was neither `asc` nor `desc`.
    #[error("unknown sort direction `{0}`, expected `asc` or `desc`")]
    UnknownDirection(String),
    /// A sort term was empty, e.g. a trailing comma.
    #[error("empty sort field")]
    EmptySortField,
    /// A sort named a field the caller does not permit.
    #[error("cannot sort by `{field}`; sortable fields are: {allowed}")]
    UnknownSortField {
        /// What the caller asked for.
        field: String,
        /// The allowlist, comma-separated.
        allowed: String,
    },
}

/// What a caller asked for: either a validated bounded window, or everything.
///
/// The fields are private so every bounded request must pass through
/// [`PageRequest::paged`] or deserialization. `try_from` on serde alone cannot
/// protect a public enum variant from direct construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageRequest {
    /// Rows to skip and return, or `None` for an unpaged request.
    window: Option<(i64, i64)>,
    /// The requested ordering.
    sort: Sort,
}

impl Serialize for PageRequest {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        #[derive(Serialize)]
        #[serde(rename_all = "snake_case")]
        enum Output<'a> {
            Paged {
                offset: i64,
                limit: i64,
                sort: &'a Sort,
            },
            Unpaged {
                sort: &'a Sort,
            },
        }

        match self.window {
            Some((offset, limit)) => Output::Paged {
                offset,
                limit,
                sort: &self.sort,
            },
            None => Output::Unpaged { sort: &self.sort },
        }
        .serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for PageRequest {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(rename_all = "snake_case")]
        enum Input {
            Paged { offset: i64, limit: i64, sort: Sort },
            Unpaged { sort: Sort },
        }

        match Input::deserialize(deserializer)? {
            Input::Paged {
                offset,
                limit,
                sort,
            } => Self::paged(offset, limit, sort).map_err(D::Error::custom),
            Input::Unpaged { sort } => Ok(Self::unpaged(sort)),
        }
    }
}

impl Default for PageRequest {
    fn default() -> Self {
        Self::unpaged(Sort::unsorted())
    }
}

impl PageRequest {
    /// A validated window, capped at [`MAX_LIMIT`].
    ///
    /// # Arguments
    ///
    /// * `offset` - How many rows to skip. Negative values are rejected.
    /// * `limit` - How many rows to return, capped at [`MAX_LIMIT`].
    /// * `sort` - The ordering. An empty [`Sort`] means the query decides.
    ///
    /// # Errors
    /// [`PageError::NegativeOffset`], [`PageError::NonPositiveLimit`] or
    /// [`PageError::LimitTooLarge`].
    pub fn paged(offset: i64, limit: i64, sort: Sort) -> Result<Self, PageError> {
        Self::paged_with_max(offset, limit, sort, MAX_LIMIT)
    }

    /// A validated window with a caller-chosen cap, for an endpoint whose rows
    /// are far larger or far smaller than average.
    ///
    /// # Arguments
    ///
    /// * `offset` - How many rows to skip. Negative values are rejected.
    /// * `limit` - How many rows to return, capped at `max`.
    /// * `sort` - The ordering. An empty [`Sort`] means the query decides.
    /// * `max` - A cap for this call site, for a route whose rows are heavier
    ///   than the default assumes.
    ///
    /// # Errors
    /// As [`PageRequest::paged`], against `max` instead of [`MAX_LIMIT`].
    pub fn paged_with_max(
        offset: i64,
        limit: i64,
        sort: Sort,
        max: i64,
    ) -> Result<Self, PageError> {
        if offset < 0 {
            return Err(PageError::NegativeOffset(offset));
        }
        if limit <= 0 {
            return Err(PageError::NonPositiveLimit(limit));
        }
        if limit > max {
            return Err(PageError::LimitTooLarge {
                requested: limit,
                max,
            });
        }
        Ok(Self {
            window: Some((offset, limit)),
            sort,
        })
    }

    /// An unbounded request.
    ///
    /// # Arguments
    ///
    /// * `sort` - The ordering to apply to the whole result set.
    #[must_use]
    pub fn unpaged(sort: Sort) -> Self {
        Self { window: None, sort }
    }

    /// The requested ordering, whether or not the request is bounded.
    #[must_use]
    pub fn sort(&self) -> &Sort {
        &self.sort
    }

    /// The offset, for a bounded request.
    #[must_use]
    pub fn offset(&self) -> Option<i64> {
        self.window.map(|(offset, _)| offset)
    }

    /// The limit, for a bounded request.
    #[must_use]
    pub fn limit(&self) -> Option<i64> {
        self.window.map(|(_, limit)| limit)
    }

    /// Borrow the `(offset, limit)` pair for query builders.
    #[must_use]
    pub fn bounds(&self) -> Option<(&i64, &i64)> {
        self.window.as_ref().map(|(offset, limit)| (offset, limit))
    }

    /// Whether this is an unbounded request.
    #[must_use]
    pub fn is_unpaged(&self) -> bool {
        self.window.is_none()
    }

    /// The next window, saturating rather than wrapping at `i64::MAX`.
    #[must_use]
    pub fn next_page(&self) -> Self {
        match self.window {
            Some((offset, limit)) => Self {
                window: Some((offset.saturating_add(limit), limit)),
                sort: self.sort.clone(),
            },
            None => self.clone(),
        }
    }

    /// The previous window, saturating at zero.
    #[must_use]
    pub fn previous_page(&self) -> Self {
        match self.window {
            Some((offset, limit)) => Self {
                window: Some((offset.saturating_sub(limit).max(0), limit)),
                sort: self.sort.clone(),
            },
            None => self.clone(),
        }
    }
}

/// One page of results, with the request that produced it and the total row
/// count that matched.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Page<T> {
    /// The loaded window.
    items: Vec<T>,
    /// The request that produced it.
    request: PageRequest,
    /// Total rows that matched, ignoring the window.
    total: i64,
}

impl<T> Page<T> {
    /// Build a page from a loaded window and the matching total.
    ///
    /// # Arguments
    ///
    /// * `items` - The rows this page holds.
    /// * `request` - The request that produced them, echoed back so a client
    ///   can build the next window without re-deriving it.
    /// * `total` - How many rows matched in total, ignoring the window.
    #[must_use]
    pub fn new(items: Vec<T>, request: PageRequest, total: i64) -> Self {
        Self {
            items,
            request,
            total,
        }
    }

    /// A page holding every row, for an unpaged request.
    ///
    /// # Arguments
    ///
    /// * `items` - Every matching row, since there is no window.
    /// * `sort` - The ordering that was applied.
    #[must_use]
    pub fn unpaged(items: Vec<T>, sort: Sort) -> Self {
        let total = i64::try_from(items.len()).unwrap_or(i64::MAX);
        Self {
            items,
            request: PageRequest::unpaged(sort),
            total,
        }
    }

    /// An empty page.
    ///
    /// # Arguments
    ///
    /// * `request` - The request that matched nothing, echoed back so the
    ///   caller still sees what was asked for.
    #[must_use]
    pub fn empty(request: PageRequest) -> Self {
        Self {
            items: Vec::new(),
            request,
            total: 0,
        }
    }

    /// The rows in this page.
    #[must_use]
    pub fn items(&self) -> &[T] {
        &self.items
    }

    /// Take the rows, dropping the metadata.
    #[must_use]
    pub fn into_items(self) -> Vec<T> {
        self.items
    }

    /// The total number of rows matching the query, not just this page.
    #[must_use]
    pub fn total(&self) -> i64 {
        self.total
    }

    /// The request that produced this page.
    #[must_use]
    pub fn request(&self) -> &PageRequest {
        &self.request
    }

    /// How many rows are in this page.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Whether this page is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// The zero-based index of this page, or `None` when unpaged.
    ///
    /// A validated request never carries a non-positive limit.
    #[must_use]
    pub fn page_number(&self) -> Option<i64> {
        self.request.window.map(|(offset, limit)| offset / limit)
    }

    /// How many pages the total spans, or `None` when unpaged.
    #[must_use]
    pub fn total_pages(&self) -> Option<i64> {
        self.request
            .limit()
            .map(|limit| self.total.saturating_add(limit - 1) / limit)
    }

    /// Whether a further page exists.
    #[must_use]
    pub fn has_next(&self) -> bool {
        self.request
            .window
            .is_some_and(|(offset, limit)| offset.saturating_add(limit) < self.total)
    }

    /// Whether an earlier page exists.
    #[must_use]
    pub fn has_previous(&self) -> bool {
        self.request.offset().is_some_and(|offset| offset > 0)
    }

    /// Convert the rows, keeping the page metadata.
    ///
    /// # Arguments
    ///
    /// * `f` - Applied to each row. The window and the total are carried over
    ///   unchanged, because converting the rows does not change what matched.
    #[must_use]
    pub fn map<U, F: FnMut(T) -> U>(self, f: F) -> Page<U> {
        Page {
            items: self.items.into_iter().map(f).collect(),
            request: self.request,
            total: self.total,
        }
    }

    /// Fallibly convert the rows, keeping the page metadata.
    ///
    /// This replaces the `into_iter().map(TryInto::try_into).collect()` plus
    /// rebuild-the-metadata block that appeared in every gRPC list handler.
    ///
    /// # Arguments
    ///
    /// * `f` - Applied to each row, stopping at the first failure. This plus
    ///   `toolbox_grpc::pagination::into_parts` is the whole entity-to-proto
    ///   conversion.
    ///
    /// # Errors
    /// The first error `f` returns.
    pub fn try_map<U, E, F: FnMut(T) -> Result<U, E>>(self, f: F) -> Result<Page<U>, E> {
        Ok(Page {
            items: self
                .items
                .into_iter()
                .map(f)
                .collect::<Result<Vec<U>, E>>()?,
            request: self.request,
            total: self.total,
        })
    }
}
