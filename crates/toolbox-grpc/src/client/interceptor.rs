//! What every outgoing request to a backend carries.

use std::time::Duration;

use secrecy::{ExposeSecret as _, SecretString};
use tonic::{
    Status,
    metadata::{Ascii, MetadataValue},
    service::Interceptor,
};
use toolbox_server::deadline::{format_grpc_timeout, time_remaining};

use crate::{X_FWD_PRINCIPAL, X_SHARED_SECRET};

/// The slack this hop keeps out of a propagated deadline: enough for the
/// backend's answer to travel back, deserialize, and be turned into a response
/// before this hop's own deadline fires.
///
/// Fixed, not a fraction of what remains: response handling costs roughly a
/// constant, and a fraction compounds over a chain (`0.9^3` is a third of the
/// budget gone), while a constant only adds.
const SLACK: Duration = Duration::from_millis(100);

tokio::task_local! {
    /// The encoded [`toolbox_auth::ForwardedPrincipal`] for the current
    /// outbound-call scope, set by [`forwarding`].
    static FORWARDED_PRINCIPAL: String;
}

/// Run `f` with `encoded` attached, as `x-fwd-principal`, to every backend call
/// it makes.
///
/// The gateway resolves the caller's principal once per inbound request and
/// wraps its fan-out in this; a call made outside any scope forwards no
/// principal.
///
/// # Arguments
///
/// * `encoded` - `toolbox_auth::ForwardedPrincipal::encode()` of the principal
///   to forward.
/// * `f` - The work whose backend calls should carry it.
pub async fn forwarding<F: Future>(encoded: String, f: F) -> F::Output {
    FORWARDED_PRINCIPAL.scope(encoded, f).await
}

/// Attaches the caller's remaining deadline, the shared service secret, and any
/// forwarded principal to every outgoing request.
///
/// An interceptor rather than a tower layer, because tonic's `Channel` is a
/// concrete type with no middleware hook: wrapping it in an `InterceptedService`
/// is how a header gets added while keeping one channel type for every backend.
#[derive(Clone)]
pub struct ClientInterceptor {
    /// The `x-shared-secret` value to attach, if one was configured.
    secret: Option<MetadataValue<Ascii>>,
}

impl ClientInterceptor {
    /// Build one from a client's configured shared secret.
    ///
    /// # Arguments
    ///
    /// * `secret` - The shared secret to present, or `None` for a client that
    ///   presents none. A secret that is not a legal header value is dropped
    ///   here rather than failing every call.
    pub(super) fn new(secret: Option<&SecretString>) -> Self {
        let secret = secret.and_then(|s| MetadataValue::try_from(s.expose_secret()).ok());
        Self { secret }
    }
}

impl std::fmt::Debug for ClientInterceptor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ClientInterceptor")
            .field("secret", &self.secret.is_some())
            .finish()
    }
}

impl Interceptor for ClientInterceptor {
    fn call(&mut self, mut request: tonic::Request<()>) -> Result<tonic::Request<()>, Status> {
        // Only when the caller had a deadline of its own. Inventing one here
        // would cap calls made outside a request, like a scheduled job's.
        if let Some(remaining) = time_remaining() {
            let budget = remaining.saturating_sub(SLACK).max(remaining / 2);
            if let Ok(value) =
                MetadataValue::try_from(format_grpc_timeout(budget).to_str().unwrap_or("0m"))
            {
                request.metadata_mut().insert("grpc-timeout", value);
            }
        }
        if let Some(secret) = &self.secret {
            request
                .metadata_mut()
                .insert(X_SHARED_SECRET, secret.clone());
        }
        if let Ok(encoded) = FORWARDED_PRINCIPAL.try_with(String::clone)
            && let Ok(value) = MetadataValue::try_from(encoded.as_str())
        {
            request.metadata_mut().insert(X_FWD_PRINCIPAL, value);
        }
        Ok(request)
    }
}
