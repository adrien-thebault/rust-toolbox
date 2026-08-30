//! The clock the process actually runs on.

use std::time::{Duration, SystemTime};

use async_trait::async_trait;

use super::Clock;

/// The real clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemClock;

#[async_trait]
impl Clock for SystemClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }

    async fn sleep(&self, d: Duration) {
        tokio::time::sleep(d).await;
    }
}
