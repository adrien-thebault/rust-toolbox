//! What every outgoing request to a backend carries.

use secrecy::ExposeSecret as _;
use tonic::{
    Status,
    metadata::{Ascii, MetadataValue},
    service::Interceptor,
};
use toolbox_server::deadline::{format_grpc_timeout, time_remaining};

use crate::auth::ServiceAuth;

/// Attaches the caller's remaining deadline and the service credential to
/// every outgoing request.
///
/// An interceptor rather than a tower layer, because tonic's `Channel` is a
/// concrete type with no middleware hook: wrapping it in an
/// `InterceptedService` is how a header gets added while keeping one channel
/// type for every backend.
#[derive(Clone)]
pub struct BackendInterceptor {
    /// The `authorization` value to attach, if a credential was configured.
    auth: Option<MetadataValue<Ascii>>,
}

impl BackendInterceptor {
    /// Build one from a backend's configured credential.
    ///
    /// # Arguments
    ///
    /// * `auth` - The credential to present, or `None` for a backend that
    ///   requires none. A secret that is not a legal header value is dropped
    ///   here rather than failing every call.
    pub(super) fn new(auth: Option<&ServiceAuth>) -> Self {
        let auth = auth
            .map(|a| match a {
                ServiceAuth::SharedSecret(secret) => secret.expose_secret().to_owned(),
            })
            .and_then(|s| MetadataValue::try_from(s.as_str()).ok());
        Self { auth }
    }
}

impl std::fmt::Debug for BackendInterceptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BackendInterceptor")
            .field("auth", &self.auth.is_some())
            .finish()
    }
}

impl Interceptor for BackendInterceptor {
    fn call(&mut self, mut request: tonic::Request<()>) -> Result<tonic::Request<()>, Status> {
        // Only when the caller had a deadline of its own. Inventing one here
        // would cap calls made outside a request, like a scheduled job's.
        if let Some(remaining) = time_remaining() {
            let budget = remaining.mul_f64(DEADLINE_SHARE);
            if let Ok(value) =
                MetadataValue::try_from(format_grpc_timeout(budget).to_str().unwrap_or("0m"))
            {
                request.metadata_mut().insert("grpc-timeout", value);
            }
        }
        if let Some(auth) = &self.auth {
            request
                .metadata_mut()
                .insert(SERVICE_AUTH_METADATA, auth.clone());
        }
        Ok(request)
    }
}

/// The share of the remaining budget a downstream call may use.
///
/// Less than all of it, so this hop keeps enough time to turn the backend's
/// answer into a response rather than timing out immediately after receiving
/// one.
const DEADLINE_SHARE: f64 = 0.9;

/// The metadata key the service credential travels under.
const SERVICE_AUTH_METADATA: &str = "x-service-auth";
