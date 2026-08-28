//! What can go wrong scheduling.

use std::collections::BTreeMap;

use toolbox_core::{ErrorKind, ServiceError};

/// A scheduling failure.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ScheduleError {
    /// A cron expression did not parse, or cannot produce another time.
    #[error("cron `{expr}`: {reason}")]
    Cron {
        /// The expression.
        expr: String,
        /// Why it failed.
        reason: String,
    },
    /// Two jobs were registered under one name.
    #[error("a job named `{0}` is already registered")]
    DuplicateName(String),
    /// No job by that name.
    #[error("no job named `{0}`")]
    NotFound(String),
    /// The lock manager failed.
    #[error("lock: {0}")]
    Lock(String),
}

impl ServiceError for ScheduleError {
    fn code(&self) -> &'static str {
        match self {
            Self::Cron { .. } => "INVALID_CRON",
            Self::DuplicateName(_) => "DUPLICATE_JOB",
            Self::NotFound(_) => "JOB_NOT_FOUND",
            Self::Lock(_) => "LOCK_FAILED",
        }
    }

    fn domain(&self) -> &'static str {
        "schedule"
    }

    fn kind(&self) -> ErrorKind {
        match self {
            Self::NotFound(_) => ErrorKind::NotFound,
            Self::Lock(_) => ErrorKind::Unavailable,
            _ => ErrorKind::InvalidArgument,
        }
    }

    fn metadata(&self) -> BTreeMap<String, String> {
        match self {
            Self::NotFound(name) | Self::DuplicateName(name) => {
                BTreeMap::from([("job".to_owned(), name.clone())])
            }
            _ => BTreeMap::new(),
        }
    }
}
