//! W3C Trace Context propagation.
//!
//! The `traceparent` header is a W3C recommendation that every tracing backend
//! already speaks, so the toolbox propagates that rather than the bespoke
//! `x-request-id` chain it used to. `x-request-id` survives only as a
//! human-quotable alias for the trace id.

mod layer;

use std::fmt;

use http::HeaderName;
pub use layer::{TraceContextLayer, TraceContextService};

/// The W3C Trace Context header.
pub const TRACEPARENT: HeaderName = HeaderName::from_static("traceparent");

/// The conventional request-id header, kept as an alias so a human can quote
/// one short string.
pub const X_REQUEST_ID: HeaderName = HeaderName::from_static("x-request-id");

/// The `&str` form of [`X_REQUEST_ID`]. The suffix says what it is, rather
/// than the old pair where only the casing distinguished them.
pub const X_REQUEST_ID_NAME: &str = "x-request-id";

/// The only `traceparent` version this parses.
const VERSION: &str = "00";

/// The `sampled` flag bit.
const FLAG_SAMPLED: u8 = 0x01;

tokio::task_local! {
    /// The trace context of the request being handled.
    ///
    /// Always set inside a request handled through [`TraceContextLayer`]: the
    /// layer mints a context when the caller did not send one, so there is one
    /// branch rather than two.
    pub static CURRENT_TRACE: TraceContext;
}

/// A W3C `traceparent`: a trace id shared by every hop, a span id for this
/// hop, and the sampling flags.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TraceContext {
    trace_id: String,
    span_id: String,
    flags: u8,
}

impl TraceContext {
    /// Mint a new root context, sampled.
    #[must_use]
    pub fn new_root() -> Self {
        Self {
            trace_id: uuid::Uuid::now_v7().simple().to_string(),
            span_id: new_span_id(),
            flags: FLAG_SAMPLED,
        }
    }

    /// Parse a `traceparent` header value.
    ///
    /// Returns `None` for any value this does not fully understand, including
    /// the all-zero trace or span ids the specification forbids. The caller
    /// mints a fresh context in that case rather than propagating something
    /// invalid.
    ///
    /// # Arguments
    ///
    /// * `value` - A `traceparent` header value. Anything not fully understood
    ///   is `None`, so a malformed header mints a fresh trace instead of
    ///   propagating a broken one.
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        let mut parts = value.trim().split('-');
        let version = parts.next()?;
        let trace_id = parts.next()?;
        let span_id = parts.next()?;
        let flags = parts.next()?;
        if parts.next().is_some() || version != VERSION {
            return None;
        }
        if trace_id.len() != 32 || !is_lower_hex(trace_id) || trace_id.bytes().all(|b| b == b'0') {
            return None;
        }
        if span_id.len() != 16 || !is_lower_hex(span_id) || span_id.bytes().all(|b| b == b'0') {
            return None;
        }
        if flags.len() != 2 || !is_lower_hex(flags) {
            return None;
        }
        Some(Self {
            trace_id: trace_id.to_owned(),
            span_id: span_id.to_owned(),
            flags: u8::from_str_radix(flags, 16).ok()?,
        })
    }

    /// A child context: same trace, a fresh span id.
    #[must_use]
    pub fn child(&self) -> Self {
        Self {
            trace_id: self.trace_id.clone(),
            span_id: new_span_id(),
            flags: self.flags,
        }
    }

    /// The trace id, shared by every hop of this request.
    #[must_use]
    pub fn trace_id(&self) -> &str {
        &self.trace_id
    }

    /// This hop's span id.
    #[must_use]
    pub fn span_id(&self) -> &str {
        &self.span_id
    }

    /// The trace id under the name a human will quote from an error page.
    #[must_use]
    pub fn request_id(&self) -> &str {
        &self.trace_id
    }

    /// Whether the caller asked for this trace to be recorded.
    #[must_use]
    pub fn is_sampled(&self) -> bool {
        self.flags & FLAG_SAMPLED != 0
    }
}

impl fmt::Display for TraceContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{VERSION}-{}-{}-{:02x}",
            self.trace_id, self.span_id, self.flags
        )
    }
}

/// Whether a string is lowercase hexadecimal, which the specification requires:
/// an uppercase `traceparent` is invalid, not merely unusual.
///
/// # Arguments
///
/// * `s` - The trace or span id to check.
fn is_lower_hex(s: &str) -> bool {
    s.bytes()
        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// A 64-bit span id, distinct for every hop.
///
/// The **trailing** eight bytes of the UUID, not the leading ones: a v7 lays
/// its millisecond timestamp in the first six, so the head carries about
/// sixteen bits of entropy and a hundred spans in one millisecond would
/// collide roughly 7% of the time. The tail is `rand_b`, which is random.
fn new_span_id() -> String {
    let bytes = uuid::Uuid::now_v7().into_bytes();
    let tail: [u8; 8] = bytes[8..].try_into().unwrap_or([0; 8]);
    format!("{:016x}", u64::from_be_bytes(tail))
}

/// The request id of the request being handled, when there is one.
///
/// Returns `None` outside a request, which is what makes this usable from code
/// that also runs at startup or from a scheduled job.
#[must_use]
pub fn current_request_id() -> Option<String> {
    CURRENT_TRACE.try_with(|c| c.request_id().to_owned()).ok()
}

/// The trace context of the request being handled, when there is one.
#[must_use]
pub fn current_trace_context() -> Option<TraceContext> {
    CURRENT_TRACE.try_with(Clone::clone).ok()
}
