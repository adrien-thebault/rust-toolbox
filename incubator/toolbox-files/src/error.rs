//! What can go wrong with a file.

use std::collections::BTreeMap;

use toolbox_core::{ErrorKind, ServiceError};

/// A failure ingesting or serving a file.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum FileError {
    /// The upload exceeded the policy's cap.
    ///
    /// Raised **while** reading, so the bytes past the cap were never held.
    #[error("upload exceeds the {max} byte limit")]
    TooLarge {
        /// The cap in force.
        max: u64,
    },
    /// The sniffed media type is not permitted.
    #[error("`{found}` is not an accepted type; accepted: {allowed}")]
    UnsupportedType {
        /// What the content actually is, not what it claimed to be.
        found: String,
        /// What would have been accepted.
        allowed: String,
    },
    /// The owner is at their quota.
    #[error("storage quota exceeded")]
    QuotaExceeded,
    /// No file with that key.
    #[error("no such file")]
    NotFound,
    /// The requested byte range is not satisfiable.
    #[error("requested range is not satisfiable for a {size} byte file")]
    RangeNotSatisfiable {
        /// The file's actual size.
        size: u64,
    },
    /// The object store failed.
    #[error("object store: {0}")]
    Store(String),
    /// The upload stream failed part way through.
    #[error("upload stream: {0}")]
    Stream(String),
}

impl From<object_store::Error> for FileError {
    fn from(e: object_store::Error) -> Self {
        match e {
            object_store::Error::NotFound { .. } => Self::NotFound,
            other => Self::Store(other.to_string()),
        }
    }
}

impl ServiceError for FileError {
    fn code(&self) -> &'static str {
        match self {
            Self::TooLarge { .. } => "FILE_TOO_LARGE",
            Self::UnsupportedType { .. } => "FILE_TYPE_NOT_ALLOWED",
            Self::QuotaExceeded => "STORAGE_QUOTA_EXCEEDED",
            Self::NotFound => "FILE_NOT_FOUND",
            Self::RangeNotSatisfiable { .. } => "RANGE_NOT_SATISFIABLE",
            Self::Store(_) | Self::Stream(_) => "FILE_STORE_FAILED",
        }
    }

    fn domain(&self) -> &'static str {
        "files"
    }

    fn kind(&self) -> ErrorKind {
        match self {
            // 413 is not in ErrorKind, and InvalidArgument is the honest
            // classification: the request was wrong, not the server.
            Self::TooLarge { .. }
            | Self::UnsupportedType { .. }
            | Self::RangeNotSatisfiable { .. } => ErrorKind::InvalidArgument,
            Self::QuotaExceeded => ErrorKind::ResourceExhausted,
            Self::NotFound => ErrorKind::NotFound,
            Self::Store(_) | Self::Stream(_) => ErrorKind::Internal,
        }
    }

    fn metadata(&self) -> BTreeMap<String, String> {
        match self {
            Self::TooLarge { max } => BTreeMap::from([("max_bytes".to_owned(), max.to_string())]),
            Self::UnsupportedType { found, allowed } => BTreeMap::from([
                ("found".to_owned(), found.clone()),
                ("allowed".to_owned(), allowed.clone()),
            ]),
            Self::RangeNotSatisfiable { size } => {
                BTreeMap::from([("size".to_owned(), size.to_string())])
            }
            _ => BTreeMap::new(),
        }
    }
}
