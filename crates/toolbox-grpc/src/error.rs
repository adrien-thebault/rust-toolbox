//! What can go wrong talking to a backend.

use std::collections::BTreeMap;

use toolbox_core::{ErrorKind, ServiceError};

/// A failure building or using a backend client.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum GrpcError {
    /// An address did not parse.
    #[error("`{0}` is not a valid URI")]
    Uri(String),
    /// A backend's addresses could not be found.
    #[error("service discovery: {0}")]
    Discovery(String),
    /// The transport could not be built or connected.
    #[error("transport: {0}")]
    Transport(String),
    /// A shared secret was configured but is empty.
    #[error("service auth for `{0}` was configured with an empty secret")]
    EmptySecret(String),
}

impl ServiceError for GrpcError {
    fn code(&self) -> &'static str {
        match self {
            Self::Uri(_) => "INVALID_BACKEND_URI",
            Self::Discovery(_) => "BACKEND_DISCOVERY_FAILED",
            Self::Transport(_) => "BACKEND_UNREACHABLE",
            Self::EmptySecret(_) => "INVALID_SERVICE_AUTH",
        }
    }

    fn domain(&self) -> &'static str {
        "grpc"
    }

    fn kind(&self) -> ErrorKind {
        match self {
            Self::Discovery(_) | Self::Transport(_) => ErrorKind::Unavailable,
            Self::Uri(_) | Self::EmptySecret(_) => ErrorKind::InvalidArgument,
        }
    }

    fn metadata(&self) -> BTreeMap<String, String> {
        BTreeMap::new()
    }
}
