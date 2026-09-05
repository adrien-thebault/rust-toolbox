//! What every outgoing request to a backend carries.

use std::time::Duration;

use secrecy::{ExposeSecret as _, SecretString};
use tonic::{
    Status,
    metadata::{Ascii, MetadataValue},
    service::Interceptor,
};
use toolbox_server::deadline::{format_grpc_timeout, time_remaining};

use crate::{X_ASSERTED_PRINCIPAL, X_SHARED_SECRET};

/// The slack this hop keeps out of a propagated deadline: enough for the
/// backend's answer to travel back, deserialize, and be turned into a response
/// before this hop's own deadline fires.
///
/// Fixed, not a fraction of what remains: response handling costs roughly a
/// constant, and a fraction compounds over a chain (`0.9^3` is a third of the
/// budget gone), while a constant only adds.
const SLACK: Duration = Duration::from_millis(100);

tokio::task_local! {
    /// The encoded [`toolbox_auth::AssertedPrincipal`] for the current
    /// outbound-call scope, set by [`asserting`].
    static ASSERTED_PRINCIPAL: String;
}

/// Run `f` with `encoded` attached, as `x-asserted-principal`, to every backend
/// call it makes.
///
/// The gateway resolves the caller's principal once per inbound request and
/// wraps its fan-out in this; a call made outside any scope asserts no
/// principal.
///
/// # Arguments
///
/// * `encoded` - `toolbox_auth::AssertedPrincipal::encode()` of the principal
///   to assert.
/// * `f` - The work whose backend calls should carry it.
pub async fn asserting<F: Future>(encoded: String, f: F) -> F::Output {
    ASSERTED_PRINCIPAL.scope(encoded, f).await
}

/// Attaches the caller's remaining deadline, the shared service secret, and any
/// asserted principal to every outgoing request.
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
        if let Ok(encoded) = ASSERTED_PRINCIPAL.try_with(String::clone)
            && let Ok(value) = MetadataValue::try_from(encoded.as_str())
        {
            request.metadata_mut().insert(X_ASSERTED_PRINCIPAL, value);
        }
        Ok(request)
    }
}

// `ClientInterceptor::new` is `pub(super)`, so an external `tests/` crate
// cannot build one - these live here instead, alongside the other
// crate-private constructs this crate unit-tests inline (see `migrate.rs` in
// `toolbox-db` for the same pattern).
#[cfg(test)]
mod tests {
    use std::time::Instant;

    use toolbox_server::DEADLINE;

    use super::*;

    fn request() -> tonic::Request<()> {
        tonic::Request::new(())
    }

    #[test]
    fn with_nothing_configured_or_in_scope_nothing_is_attached() {
        let mut interceptor = ClientInterceptor::new(None);
        let req = interceptor.call(request()).unwrap();
        assert!(req.metadata().get("grpc-timeout").is_none());
        assert!(req.metadata().get(X_SHARED_SECRET).is_none());
        assert!(req.metadata().get(X_ASSERTED_PRINCIPAL).is_none());
    }

    #[test]
    fn a_configured_secret_is_attached() {
        let mut interceptor = ClientInterceptor::new(Some(&SecretString::from("s3cr3t")));
        let req = interceptor.call(request()).unwrap();
        assert_eq!(req.metadata().get(X_SHARED_SECRET).unwrap(), "s3cr3t");
    }

    /// A newline is illegal in a header value; the doc on `new` promises this
    /// is dropped rather than failing every call.
    #[test]
    fn a_secret_that_is_not_a_legal_header_value_is_dropped() {
        let mut interceptor = ClientInterceptor::new(Some(&SecretString::from("bad\nvalue")));
        let req = interceptor.call(request()).unwrap();
        assert!(req.metadata().get(X_SHARED_SECRET).is_none());
    }

    #[test]
    fn a_scoped_deadline_becomes_a_grpc_timeout() {
        let mut interceptor = ClientInterceptor::new(None);
        let req = DEADLINE.sync_scope(Instant::now() + Duration::from_secs(5), || {
            interceptor.call(request())
        });
        assert!(req.unwrap().metadata().get("grpc-timeout").is_some());
    }

    #[tokio::test]
    async fn an_asserting_scope_attaches_the_encoded_principal() {
        let mut interceptor = ClientInterceptor::new(None);
        let req = asserting("the-encoded-principal".to_owned(), async {
            interceptor.call(request())
        })
        .await
        .unwrap();
        assert_eq!(
            req.metadata().get(X_ASSERTED_PRINCIPAL).unwrap(),
            "the-encoded-principal"
        );
    }
}
