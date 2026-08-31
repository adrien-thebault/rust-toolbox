//! Building a client for another service.
//!
//! Connecting to a backend correctly means a timeout, message limits that match
//! the server's, deadline propagation, and a shared credential - decisions each
//! with a bad default, repeated per backend per project.
//!
//! Point `uri` at your load balancer, mesh, or Kubernetes `Service`. Client-side
//! discovery is deliberately absent: it is the infrastructure's job, and
//! re-resolving DNS in-process to work around an L4 `ClusterIP` is a workaround
//! better solved by putting a proxy in front. Note that `connect_lazy` on a
//! name that resolves to several addresses pins to whichever answered first.

pub mod error;
pub mod interceptor;
pub mod retry;

use std::time::Duration;

pub use error::ClientError;
pub use interceptor::{ClientInterceptor, forwarding};
pub use retry::{Backoff, RetryPolicy, is_retryable, with_retry};
use secrecy::SecretString;
use tonic::{
    service::interceptor::InterceptedService,
    transport::{Channel, Endpoint, Uri},
};

use crate::limits::MessageLimits;

/// Everything needed to reach one backend.
#[derive(Debug, Clone)]
pub struct ClientConfig {
    /// The backend's address. Parsed at construction, so a typo is a startup
    /// error rather than a first-call one.
    pub uri: Uri,
    /// How long to wait for a connection.
    pub connect_timeout: Duration,
    /// How long a single call may take, if the caller has no deadline of its
    /// own.
    pub request_timeout: Option<Duration>,
    /// HTTP/2 keepalive interval, so a silently dropped connection is noticed
    /// before the next request finds it.
    pub keepalive: Option<Duration>,
    /// Message size limits, shared with the server.
    pub limits: MessageLimits,
    /// Whether and which calls may be retried.
    pub retry: RetryPolicy,
    /// The shared secret this client presents, proving it is an allowed caller.
    pub service_secret: Option<SecretString>,
}

impl ClientConfig {
    /// A config for one address, with defaults everywhere else.
    ///
    /// # Arguments
    ///
    /// * `uri` - The backend's address, `http://host:port`.
    ///
    /// # Errors
    /// [`ClientError::Uri`] when `uri` does not parse.
    pub fn new(uri: &str) -> Result<Self, ClientError> {
        Ok(Self {
            uri: uri.parse().map_err(|_| ClientError::Uri(uri.to_owned()))?,
            connect_timeout: Duration::from_secs(5),
            request_timeout: Some(Duration::from_secs(30)),
            keepalive: Some(Duration::from_secs(30)),
            limits: MessageLimits::default(),
            retry: RetryPolicy::None,
            service_secret: None,
        })
    }

    /// Present a shared service secret on every call.
    ///
    /// # Arguments
    ///
    /// * `secret` - The secret the backend's `shared_secret_layer` checks, so
    ///   it can tell an allowed caller from anything else that reached the
    ///   network.
    #[must_use]
    pub fn service_secret(mut self, secret: impl Into<String>) -> Self {
        self.service_secret = Some(SecretString::from(secret.into()));
        self
    }

    /// Allow retries on the named idempotent methods.
    ///
    /// # Arguments
    ///
    /// * `retry` - Which methods may be retried, and how often. Naming methods
    ///   rather than retrying everything is what keeps a non-idempotent call
    ///   from running twice.
    #[must_use]
    pub fn retry(mut self, retry: RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    /// Set the message size limits.
    ///
    /// # Arguments
    ///
    /// * `limits` - The largest message to encode and decode. The same value
    ///   has to be applied at both ends, because tonic puts it on the generated
    ///   client and server types separately.
    #[must_use]
    pub fn limits(mut self, limits: MessageLimits) -> Self {
        self.limits = limits;
        self
    }

    /// Build an endpoint for this config's `uri`, with its timeouts applied.
    fn endpoint(&self) -> Endpoint {
        let mut endpoint = Channel::builder(self.uri.clone()).connect_timeout(self.connect_timeout);
        if let Some(timeout) = self.request_timeout {
            endpoint = endpoint.timeout(timeout);
        }
        if let Some(interval) = self.keepalive {
            endpoint = endpoint
                .http2_keep_alive_interval(interval)
                .keep_alive_while_idle(true);
        }
        endpoint
    }
}

/// A connected backend.
///
/// **One concrete channel type for every backend**, so a generated client's
/// `new` never needs an `as fn(_) -> _` cast at one call site and not another.
#[derive(Debug, Clone)]
pub struct ClientChannel {
    /// The backend's name, for spans and errors.
    name: &'static str,
    /// The underlying tonic channel.
    channel: Channel,
    /// Encoded-message size limits.
    limits: MessageLimits,
    /// When and how a call is retried.
    retry: RetryPolicy,
    /// Adds the outbound secret, forwarded principal and deadline headers.
    interceptor: ClientInterceptor,
}

/// What a generated client is built over.
pub type ClientService = InterceptedService<Channel, ClientInterceptor>;

impl ClientChannel {
    /// The channel, to hand to a generated client.
    ///
    /// Carries the deadline-propagation, shared-secret and forwarded-principal
    /// interceptor, so a gateway that times out does not leave this backend
    /// working on a request nobody is waiting for.
    #[must_use]
    pub fn channel(&self) -> ClientService {
        InterceptedService::new(self.channel.clone(), self.interceptor.clone())
    }

    /// The bare channel, without the interceptor.
    ///
    /// For a caller that needs to compose its own middleware. Note that using it
    /// gives up deadline propagation, service auth and identity forwarding.
    #[must_use]
    pub fn raw_channel(&self) -> Channel {
        self.channel.clone()
    }

    /// The retry policy this backend was configured with.
    ///
    /// Applied through [`with_retry`], not automatically: tonic offers no
    /// generic per-call retry hook, and a policy that silently did nothing
    /// would be worse than none.
    #[must_use]
    pub fn retry(&self) -> &RetryPolicy {
        &self.retry
    }

    /// The name this backend was built under, for logs.
    #[must_use]
    pub fn name(&self) -> &'static str {
        self.name
    }

    /// The limits to apply to a generated client, so both ends match.
    #[must_use]
    pub fn limits(&self) -> MessageLimits {
        self.limits
    }
}

/// Connect to a backend.
///
/// Connection is lazy: this returns without a round trip, so a process starts
/// even when a dependency is briefly down, and the first call is what fails.
///
/// # Arguments
///
/// * `name` - The backend's label in spans, metrics and errors. A `&'static
///   str`, so it cannot carry per-request data.
/// * `cfg` - Address, limits, deadline share and secret for this backend.
#[must_use]
pub fn client(name: &'static str, cfg: &ClientConfig) -> ClientChannel {
    ClientChannel {
        name,
        channel: cfg.endpoint().connect_lazy(),
        limits: cfg.limits,
        retry: cfg.retry.clone(),
        interceptor: ClientInterceptor::new(cfg.service_secret.as_ref()),
    }
}
