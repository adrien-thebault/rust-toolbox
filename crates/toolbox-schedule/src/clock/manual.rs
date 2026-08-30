//! A clock a test drives by hand.

use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;

use super::Clock;

/// A clock that only moves when a test moves it.
#[derive(Debug, Clone)]
pub struct ManualClock {
    /// The current time, milliseconds since an arbitrary epoch.
    millis: Arc<AtomicU64>,
    /// Woken on every advance so pending sleeps re-check the time.
    tick: Arc<tokio::sync::Notify>,
}

impl Default for ManualClock {
    fn default() -> Self {
        Self::new()
    }
}

impl ManualClock {
    /// A clock stopped at the Unix epoch.
    #[must_use]
    pub fn new() -> Self {
        Self {
            millis: Arc::new(AtomicU64::new(0)),
            tick: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// A clock stopped at `t`.
    ///
    /// # Arguments
    ///
    /// * `t` - The instant the clock reports until a test advances it.
    #[must_use]
    pub fn at(t: SystemTime) -> Self {
        let millis = t
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX));
        Self {
            millis: Arc::new(AtomicU64::new(millis)),
            tick: Arc::new(tokio::sync::Notify::new()),
        }
    }

    /// Move the clock forward and wake anything sleeping.
    ///
    /// # Arguments
    ///
    /// * `d` - How far to move forward. Anything sleeping past the new instant
    ///   is woken.
    pub fn advance(&self, d: Duration) {
        let by = u64::try_from(d.as_millis()).unwrap_or(u64::MAX);
        self.millis.fetch_add(by, Ordering::SeqCst);
        self.tick.notify_waiters();
    }

    /// Milliseconds since the epoch, for an assertion.
    #[must_use]
    pub fn millis(&self) -> u64 {
        self.millis.load(Ordering::SeqCst)
    }
}

#[async_trait]
impl Clock for ManualClock {
    fn now(&self) -> SystemTime {
        UNIX_EPOCH + Duration::from_millis(self.millis.load(Ordering::SeqCst))
    }

    async fn sleep(&self, d: Duration) {
        let deadline = self
            .millis
            .load(Ordering::SeqCst)
            .saturating_add(u64::try_from(d.as_millis()).unwrap_or(u64::MAX));

        let notified = self.tick.notified();
        tokio::pin!(notified);
        // Enable before the first deadline check: a wake that lands in the gap
        // between the check and the await is then remembered rather than lost,
        // which is what `notify_waiters` does not buffer on its own.
        notified.as_mut().enable();
        while self.millis.load(Ordering::SeqCst) < deadline {
            notified.as_mut().await;
            notified.set(self.tick.notified());
            notified.as_mut().enable();
        }
    }
}
