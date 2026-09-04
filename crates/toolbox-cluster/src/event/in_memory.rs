//! The default bus: a tokio broadcast channel per topic.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use async_trait::async_trait;
use tokio::sync::broadcast;
use tokio_stream::StreamExt as _;

use super::{CloudEvent, EventBus, EventBusError, EventStream, Topic};

/// The default bus: a tokio broadcast channel per topic.
///
/// **Single replica only.** Events published on this instance never reach a
/// subscriber on another, so under more than one replica a subscriber misses
/// most of the stream. Use a shared adapter once you are running more than
/// one.
///
/// **At-most-once, and not durable.** A subscriber that falls behind `buffer`
/// events loses the ones it missed rather than blocking the publisher; there
/// is no history to replay once it is gone.
pub struct InMemoryEventBus {
    /// One broadcast sender per topic, created on first use.
    topics: Mutex<HashMap<Topic, broadcast::Sender<CloudEvent>>>,
    /// Per-topic channel capacity; a slow subscriber past this lags.
    buffer: usize,
}

impl std::fmt::Debug for InMemoryEventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InMemoryEventBus")
            .field("buffer", &self.buffer)
            .finish_non_exhaustive()
    }
}

impl Default for InMemoryEventBus {
    fn default() -> Self {
        Self::new(1024)
    }
}

impl InMemoryEventBus {
    /// A bus buffering `buffer` events per topic for slow subscribers.
    ///
    /// # Arguments
    ///
    /// * `buffer` - How many events to hold per topic for a subscriber that has
    ///   fallen behind. Past it, the slow subscriber loses events rather than
    ///   the publisher blocking. Clamped to at least 1, since a zero-capacity
    ///   broadcast channel cannot be constructed.
    #[must_use]
    pub fn new(buffer: usize) -> Self {
        Self {
            topics: Mutex::new(HashMap::new()),
            buffer: buffer.max(1),
        }
    }

    /// The broadcast channel for a topic, created on first use.
    ///
    /// # Arguments
    ///
    /// * `topic` - The topic whose channel is wanted.
    fn sender(&self, topic: &Topic) -> broadcast::Sender<CloudEvent> {
        let mut topics = self.topics.lock().unwrap_or_else(PoisonError::into_inner);
        topics
            .entry(topic.clone())
            .or_insert_with(|| broadcast::channel(self.buffer).0)
            .clone()
    }
}

#[async_trait]
impl EventBus for InMemoryEventBus {
    async fn publish(&self, topic: &Topic, event: CloudEvent) -> Result<(), EventBusError> {
        // An error here means nobody is subscribed, which is not a failure.
        let _ = self.sender(topic).send(event);
        Ok(())
    }

    async fn subscribe(&self, topic: &Topic) -> Result<EventStream, EventBusError> {
        let rx = self.sender(topic).subscribe();
        let stream =
            tokio_stream::wrappers::BroadcastStream::new(rx).filter_map(std::result::Result::ok);
        Ok(Box::pin(stream))
    }
}
