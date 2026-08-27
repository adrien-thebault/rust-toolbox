//! The clock, as a trait.
//!
//! `toolbox-schedule` decides when to run a job by asking something for the
//! time, and a scheduler that asks `SystemTime` directly can only be tested by
//! sleeping - which is how a test suite ends up taking four minutes to assert
//! one cron expression. Two implementations ship here, so this is a trait
//! rather than a function.

mod manual;
mod system;

use std::time::{Duration, SystemTime};

use async_trait::async_trait;
pub use manual::ManualClock;
pub use system::SystemClock;

/// A source of the current time that can be driven by a test.
#[async_trait]
pub trait Clock: Send + Sync {
    /// The current wall-clock time.
    fn now(&self) -> SystemTime;

    /// Wait for `d`. Under a [`ManualClock`] this returns only once the clock
    /// has been advanced past the deadline.
    ///
    /// # Arguments
    ///
    /// * `d` - How long to wait. Under a [`ManualClock`] it is measured against
    ///   the clock the test drives, not against real time.
    async fn sleep(&self, d: Duration);
}
