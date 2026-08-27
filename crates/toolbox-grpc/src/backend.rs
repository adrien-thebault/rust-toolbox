//! Building a client for another service.
//!
//! Connecting to a backend correctly means a timeout, message limits that match
//! the server's, deadline propagation, and a discovery strategy - four
//! decisions, each with a bad default, repeated per backend per project.

mod dns;
mod interceptor;

use std::time::Duration;

use dns::dns_channel;
pub use interceptor::BackendInterceptor;
use tonic::{
    service::interceptor::InterceptedService,
    transport::{Channel, Endpoint},
};

use crate::{
    auth::ServiceAuth,
    discovery::{Discovery, EndpointSettings},
    error::GrpcError,
    retry::RetryPolicy,
};

/// The largest messages a client or server will encode and decode.
///
/// One value read by both ends. Neither end can apply it for you - tonic puts
/// `max_decoding_message_size` on the generated client and server types, with
/// no trait to reach it through - so a caller passes it to both from the same
/// `MessageLimits`. That turns a silent drift into a visible one: the two ends
/// differ only if somebody passes different values, not by forgetting one.
#[derive(Debug, Clone, Copy)]
pub struct MessageLimits {
    /// The largest message this end will decode.
    pub max_decoding: usize,
    /// The largest message this end will encode.
    pub max_encoding: usize,
}

impl Default for MessageLimits {
    fn default() -> Self {
        // tonic's own default is 4 MiB for decoding and unlimited for
        // encoding, which is the asymmetry that produces "it works from the
        // gateway but not from the backend".
        Self {
            max_decoding: 4 * 1024 * 1024,
            max_encoding: 4 * 1024 * 1024,
        }
    }
}

/// Everything needed to reach one backend.
#[derive(Debug, Clone)]
pub struct BackendConfig {
    /// How to find it.
    pub discovery: Discovery,
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
    /// The credential this client presents.
    pub auth: Option<ServiceAuth>,
}

impl BackendConfig {
    /// A config for one address, with defaults everywhere else.
    ///
    /// # Arguments
    ///
    /// * `uri` - The backend's address, `http://host:port`. Parsed now, so a
    ///   typo is a startup error rather than a first-call one.
    ///
    /// # Errors
    /// [`GrpcError::Uri`] when `uri` does not parse.
    pub fn new(uri: &str) -> Result<Self, GrpcError> {
        Ok(Self {
            discovery: Discovery::single(uri)?,
            connect_timeout: Duration::from_secs(5),
            request_timeout: Some(Duration::from_secs(30)),
            keepalive: Some(Duration::from_secs(30)),
            limits: MessageLimits::default(),
            retry: RetryPolicy::None,
            auth: None,
        })
    }

    /// Use a different discovery strategy.
    ///
    /// # Arguments
    ///
    /// * `discovery` - How to find the backend. A fixed address on a laptop,
    ///   DNS in a cluster, and only the wiring changes.
    #[must_use]
    pub fn discovery(mut self, discovery: Discovery) -> Self {
        self.discovery = discovery;
        self
    }

    /// Present a service credential.
    ///
    /// # Arguments
    ///
    /// * `auth` - The credential to attach to every outgoing request, so a
    ///   backend can tell its gateway from anything else that reached the
    ///   network.
    #[must_use]
    pub fn auth(mut self, auth: ServiceAuth) -> Self {
        self.auth = Some(auth);
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

    /// The endpoint settings every discovered address gets.
    #[must_use]
    pub fn endpoint_settings(&self) -> EndpointSettings {
        EndpointSettings {
            connect_timeout: self.connect_timeout,
            request_timeout: self.request_timeout,
            keepalive: self.keepalive,
        }
    }
}

/// A connected backend.
///
/// **One concrete channel type for every backend**, whatever the discovery
/// strategy, so a generated client's `new` never needs an `as fn(_) -> _` cast
/// at one call site and not another.
#[derive(Debug, Clone)]
pub struct BackendChannel {
    name: &'static str,
    channel: Channel,
    limits: MessageLimits,
    retry: RetryPolicy,
    interceptor: BackendInterceptor,
}

/// What a generated client is built over.
pub type BackendService = InterceptedService<Channel, BackendInterceptor>;

impl BackendChannel {
    /// The channel, to hand to a generated client.
    ///
    /// Carries the deadline-propagation and service-auth interceptor, so a
    /// gateway that times out does not leave this backend working on a request
    /// nobody is waiting for.
    #[must_use]
    pub fn channel(&self) -> BackendService {
        InterceptedService::new(self.channel.clone(), self.interceptor.clone())
    }

    /// The bare channel, without the interceptor.
    ///
    /// For a caller that needs to compose its own middleware. Note that using
    /// it gives up deadline propagation and service auth.
    #[must_use]
    pub fn raw_channel(&self) -> Channel {
        self.channel.clone()
    }

    /// The retry policy this backend was configured with.
    ///
    /// Applied through [`crate::retry::with_retry`], not automatically: tonic
    /// offers no generic per-call retry hook, and a policy that silently did
    /// nothing would be worse than none.
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
/// * `cfg` - Discovery, limits, deadline share and credential for this backend.
///
/// # Errors
/// [`GrpcError::Discovery`] when a DNS name resolves to nothing, or
/// [`GrpcError::Transport`] when an endpoint cannot be built.
pub async fn backend(name: &'static str, cfg: &BackendConfig) -> Result<BackendChannel, GrpcError> {
    let settings = cfg.endpoint_settings();
    let channel = match &cfg.discovery {
        Discovery::Proxy(uri) => settings.endpoint(uri.clone()).connect_lazy(),
        Discovery::Static(uris) => {
            if uris.is_empty() {
                return Err(GrpcError::Discovery(format!("`{name}` has no addresses")));
            }
            let endpoints: Vec<Endpoint> =
                uris.iter().map(|u| settings.endpoint(u.clone())).collect();
            Channel::balance_list(endpoints.into_iter())
        }
        Discovery::Dns {
            name: host,
            port,
            refresh,
        } => dns_channel(name, host, *port, *refresh, settings).await?,
    };

    Ok(BackendChannel {
        name,
        channel,
        limits: cfg.limits,
        retry: cfg.retry.clone(),
        interceptor: BackendInterceptor::new(cfg.auth.as_ref()),
    })
}
