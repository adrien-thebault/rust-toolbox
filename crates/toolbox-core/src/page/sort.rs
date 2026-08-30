//! Sort terms: a field name and a direction, validated against an allowlist.

use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

use super::PageError;

/// Which way a sort term orders.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SortDirection {
    /// Ascending.
    Asc,
    /// Descending.
    Desc,
}

impl SortDirection {
    /// The SQL keyword for this direction.
    #[must_use]
    pub fn as_sql(self) -> &'static str {
        match self {
            Self::Asc => "ASC",
            Self::Desc => "DESC",
        }
    }
}

impl fmt::Display for SortDirection {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Asc => "asc",
            Self::Desc => "desc",
        })
    }
}

/// One term of a sort: a field name and a direction.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SortItem {
    /// The field to order by. Validated against an allowlist before it ever
    /// reaches SQL.
    pub field: String,
    /// The direction.
    pub direction: SortDirection,
}

impl SortItem {
    /// An ascending term.
    ///
    /// # Arguments
    ///
    /// * `field` - The column name to sort on. It is validated against the
    ///   entity's allowlist before it reaches any SQL.
    pub fn asc(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            direction: SortDirection::Asc,
        }
    }

    /// A descending term.
    ///
    /// # Arguments
    ///
    /// * `field` - The column name to sort on, descending. Validated the same
    ///   way as `asc`.
    pub fn desc(field: impl Into<String>) -> Self {
        Self {
            field: field.into(),
            direction: SortDirection::Desc,
        }
    }
}

impl fmt::Display for SortItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.direction == SortDirection::Desc {
            f.write_str("-")?;
        }
        f.write_str(&self.field)
    }
}

/// An ordered list of sort terms. Empty means unsorted.
///
/// One representation, not a `Sorted | Unsorted` enum: the enum forced every
/// consumer to match on a case that behaves identically to an empty list.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Sort(Vec<SortItem>);

impl Sort {
    /// An unsorted request.
    #[must_use]
    pub fn unsorted() -> Self {
        Self(Vec::new())
    }

    /// Build from terms.
    ///
    /// # Arguments
    ///
    /// * `items` - The sort terms, in priority order: the first breaks ties
    ///   last.
    #[must_use]
    pub fn new(items: Vec<SortItem>) -> Self {
        Self(items)
    }

    /// The terms, in order.
    #[must_use]
    pub fn items(&self) -> &[SortItem] {
        &self.0
    }

    /// Whether no ordering was requested.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// How many terms.
    #[must_use]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Append a term.
    ///
    /// # Arguments
    ///
    /// * `item` - The next tie-break term, applied after every term already
    ///   present.
    #[must_use]
    pub fn then(mut self, item: SortItem) -> Self {
        self.0.push(item);
        self
    }

    /// Parse the compact form: `-created_at,title` is `created_at` descending
    /// then title ascending.
    ///
    /// Round-trips with `Display`, which is what lets one representation serve
    /// a query string, a protobuf field and a SQL `ORDER BY`.
    ///
    /// # Arguments
    ///
    /// * `s` - A comma-separated list of terms, each a field name optionally
    ///   prefixed with `-` for descending or `+` for ascending, e.g.
    ///   `-created_at,title`. No prefix means ascending.
    ///
    /// # Errors
    /// [`PageError::EmptySortField`] when a term is blank.
    pub fn parse(s: &str) -> Result<Self, PageError> {
        if s.trim().is_empty() {
            return Ok(Self::unsorted());
        }
        let mut items = Vec::new();
        for raw in s.split(',') {
            let term = raw.trim();
            let (direction, field) = match term.strip_prefix('-') {
                Some(rest) => (SortDirection::Desc, rest.trim()),
                None => (
                    SortDirection::Asc,
                    term.strip_prefix('+').unwrap_or(term).trim(),
                ),
            };
            if field.is_empty() {
                return Err(PageError::EmptySortField);
            }
            items.push(SortItem {
                field: field.to_owned(),
                direction,
            });
        }
        Ok(Self(items))
    }
}

impl FromStr for Sort {
    type Err = PageError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::parse(s)
    }
}

impl fmt::Display for Sort {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, item) in self.0.iter().enumerate() {
            if i > 0 {
                f.write_str(",")?;
            }
            write!(f, "{item}")?;
        }
        Ok(())
    }
}

impl Sort {
    /// Reject any term naming a field that is not in `allowed`.
    ///
    /// The check that stands between a query parameter and SQL injection, so
    /// it lives on `Sort` itself rather than somewhere a caller has to
    /// remember to reach for. Applying the ordering needs the column types and
    /// stays with whatever generated the query.
    ///
    /// # Arguments
    ///
    /// * `allowed` - The field names the caller is willing to sort on. Anything
    ///   else is rejected rather than interpolated into SQL.
    ///
    /// # Errors
    /// [`PageError::UnknownSortField`] naming the offending field and the
    /// allowlist.
    pub fn validate(&self, allowed: &[&str]) -> Result<(), PageError> {
        for item in &self.0 {
            if !allowed.contains(&item.field.as_str()) {
                return Err(PageError::UnknownSortField {
                    field: item.field.clone(),
                    allowed: allowed.join(", "),
                });
            }
        }
        Ok(())
    }
}

impl IntoIterator for Sort {
    type Item = SortItem;
    type IntoIter = std::vec::IntoIter<SortItem>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}
