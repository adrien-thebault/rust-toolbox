//! Retry policy for backend calls.
//!
//! The default has to be "do not retry", and the methods that may be retried
//! have to be listed. A policy that silently retries every call duplicates a
//! `create_registration` the first time a response is lost after the server
//! committed.

use std::time::Duration;

use tracing::debug;

/// Exponential backoff parameters: the first wait, the ceiling, the multiplier
/// applied to each successive wait, and whether to jitter.
#[derive(Debug, Clone, Copy)]
pub struct Backoff {
    /// The first wait.
    pub min_delay: Duration,
    /// The longest wait.
    pub max_delay: Duration,
    /// The multiplier applied to each successive wait.
    pub factor: f32,
    /// Whether to jitter, so a fleet of clients does not retry in lockstep.
    pub jitter: bool,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            min_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(5),
            factor: 2.0,
            jitter: true,
        }
    }
}

/// Whether, and which, calls may be retried.
#[derive(Debug, Clone, Default)]
pub enum RetryPolicy {
    /// Never retry. The default, and the right answer unless you have thought
    /// about it.
    #[default]
    None,
    /// Retry the named methods, which the caller asserts are idempotent.
    Idempotent {
        /// How many attempts in total, including the first.
        max_attempts: usize,
        /// How long to wait between them.
        backoff: Backoff,
        /// The bare method names that may be retried, e.g. `["GetEvent"]`.
        methods: &'static [&'static str],
    },
}

impl RetryPolicy {
    /// Whether `method` may be retried under this policy.
    ///
    /// # Arguments
    ///
    /// * `method` - The bare gRPC method name, as it appears in the service
    ///   definition.
    #[must_use]
    pub fn allows(&self, method: &str) -> bool {
        match self {
            Self::None => false,
            Self::Idempotent { methods, .. } => methods.contains(&method),
        }
    }

    /// How many attempts `method` gets.
    ///
    /// # Arguments
    ///
    /// * `method` - The bare gRPC method name. A method the policy does not
    ///   name gets one attempt, never zero.
    #[must_use]
    pub fn attempts(&self, method: &str) -> usize {
        match self {
            Self::Idempotent { max_attempts, .. } if self.allows(method) => *max_attempts,
            // Every other case is a single attempt: no policy, or a method
            // this policy does not cover.
            _ => 1,
        }
    }
}

/// Run a call under a retry policy.
///
/// Explicit rather than hidden in the channel: tonic has no per-call retry hook,
/// and a unary call consumes its request, so only the caller can rebuild it for
/// a retry. Only [`is_retryable`] codes are retried.
///
/// ```ignore
/// let todo = with_retry(channel.retry(), "GetTodo", || async {
///     client.clone().get_todo(GetTodoRequest { id }).await
/// })
/// .await?;
/// ```
///
/// # Arguments
///
/// * `policy` - Which methods may be retried, and how often.
/// * `method` - The method being called, matched against the policy.
/// * `operation` - Builds and runs the call. It is a closure rather than a
///   value because a unary call consumes its request, so a retry needs a fresh
///   one.
///
/// # Errors
/// The last error the operation returned.
pub async fn with_retry<T, F, Fut>(
    policy: &RetryPolicy,
    method: &str,
    mut operation: F,
) -> Result<T, tonic::Status>
where
    F: FnMut() -> Fut,
    Fut: Future<Output = Result<T, tonic::Status>>,
{
    let attempts = policy.attempts(method);
    let RetryPolicy::Idempotent { backoff, .. } = policy else {
        return operation().await;
    };

    let mut delay = backoff.min_delay;
    let mut last = None;

    for attempt in 1..=attempts {
        match operation().await {
            Ok(value) => return Ok(value),
            Err(status) if attempt < attempts && is_retryable(status.code()) => {
                debug!(
                    method,
                    attempt,
                    code = ?status.code(),
                    "retrying a failed call"
                );
                tokio::time::sleep(jitter(delay, backoff.jitter)).await;
                delay = (delay.mul_f32(backoff.factor)).min(backoff.max_delay);
                last = Some(status);
            }
            Err(status) => return Err(status),
        }
    }
    Err(last.unwrap_or_else(|| tonic::Status::unavailable("retries exhausted")))
}

/// Whether asking again could plausibly give a different answer.
///
/// # Arguments
///
/// * `code` - The status the call failed with. `Unavailable` may succeed on a
///   second try; `InvalidArgument` never will.
#[must_use]
pub fn is_retryable(code: tonic::Code) -> bool {
    matches!(
        code,
        tonic::Code::Unavailable | tonic::Code::DeadlineExceeded | tonic::Code::ResourceExhausted
    )
}

/// Spread retries out so a fleet of clients does not reconnect in lockstep.
///
/// # Arguments
///
/// * `delay` - The backoff computed for this attempt.
/// * `enabled` - Whether to spread it. Off makes a test deterministic; on is
///   what stops a fleet reconnecting in lockstep.
fn jitter(delay: Duration, enabled: bool) -> Duration {
    if !enabled {
        return delay;
    }
    // Half to full, which is the standard "full jitter" shape. A real random
    // byte: the first byte of a v7 uuid is the top of its millisecond clock,
    // which would make this a constant for decades at a time.
    let mut byte = [0u8; 1];
    getrandom::fill(&mut byte).expect("the OS random source");
    delay.mul_f64(0.5 + f64::from(byte[0]) / 255.0 * 0.5)
}
