//! Bridging a validated sort into this crate's error type.

use toolbox_core::{PageError, Sort};

use crate::error::{DbError, DbResult};

/// Reject any sort term the entity did not declare sortable.
///
/// [`Sort::validate`] does the checking; this turns its error into a
/// [`DbError`] so a generated query method returns one error type.
///
/// # Arguments
///
/// * `sort` - The terms the caller asked for, straight from the query string.
/// * `allowed` - The field names the entity declared sortable. Anything outside
///   it is an error, never interpolated SQL.
///
/// # Errors
/// [`DbError::InvalidSortField`] naming the offending field and the allowlist.
pub fn validate(sort: &Sort, allowed: &[&str]) -> DbResult<()> {
    sort.validate(allowed).map_err(|e| match e {
        PageError::UnknownSortField { field, allowed } => {
            DbError::InvalidSortField { field, allowed }
        }
        other => DbError::InvalidSortField {
            field: other.to_string(),
            allowed: allowed.join(", "),
        },
    })
}
