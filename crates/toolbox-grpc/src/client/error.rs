//! What can go wrong building or using a client.

use std::collections::BTreeMap;

use toolbox_core::{ErrorKind, ServiceError};

/// A failure building a [`ClientChannel`](super::ClientChannel).
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ClientError {
    /// An address did not parse.
    #[error("`{0}` is not a valid URI")]
    Uri(String),
    /// The transport could not be built.
    #[error("transport: {0}")]
    Transport(String),
}

impl ServiceError for ClientError {
    fn code(&self) -> &'static str {
        match self {
            Self::Uri(_) => "INVALID_BACKEND_URI",
            Self::Transport(_) => "BACKEND_UNREACHABLE",
        }
    }

    fn domain(&self) -> &'static str {
        "grpc"
    }

    fn kind(&self) -> ErrorKind {
        match self {
            Self::Transport(_) => ErrorKind::Unavailable,
            Self::Uri(_) => ErrorKind::InvalidArgument,
        }
    }

    fn metadata(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }
}
