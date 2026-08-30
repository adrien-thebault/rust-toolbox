//! The default bus: a tokio broadcast channel per topic.

use std::{
    collections::HashMap,
    sync::{Mutex, PoisonError},
};

use async_trait::async_trait;
use tokio::sync::broadcast;
use tokio_stream::StreamExt as _;

use super::{
    BusOrdering, CloudEvent, Delivery, EventBus, EventBusCapabilities, EventBusError, EventStream,
    StartPosition, Topic,
};
use crate::deployment::{Adapter, Scope};

/// The default bus: a tokio broadcast channel per topic.
///
/// **Single replica only.** Events published on this instance never reach a
/// subscriber on another, so under `DEPLOYMENT=clustered` a subscriber misses
/// most of the stream. That is why it declares [`Scope::Local`] and the
/// startup guard refuses to run it clustered.
pub struct InProcessEventBus {
    /// One broadcast sender per topic, created on first use.
    topics: Mutex<HashMap<Topic, broadcast::Sender<CloudEvent>>>,
    /// Per-topic channel capacity; a slow subscriber past this lags.
    buffer: usize,
}

impl std::fmt::Debug for InProcessEventBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InProcessEventBus")
            .field("buffer", &self.buffer)
            .finish_non_exhaustive()
    }
}

impl Default for InProcessEventBus {
    fn default() -> Self {
        Self::new(1024)
    }
}

impl InProcessEventBus {
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
impl EventBus for InProcessEventBus {
    fn capabilities(&self) -> EventBusCapabilities {
        EventBusCapabilities {
            delivery: Delivery::AtMostOnce,
            replay: None,
            ordering: BusOrdering::PerTopic,
            max_payload: usize::MAX,
            durable: false,
        }
    }

    async fn publish(&self, topic: &Topic, event: CloudEvent) -> Result<(), EventBusError> {
        // An error here means nobody is subscribed, which is not a failure.
        let _ = self.sender(topic).send(event);
        Ok(())
    }

    async fn subscribe(
        &self,
        topic: &Topic,
        from: StartPosition,
    ) -> Result<EventStream, EventBusError> {
        // No replay: only `Now` is honest here. `Earliest` used to be accepted
        // and then silently behave like `Now`, since a fresh receiver holds no
        // history.
        self.capabilities().check_start(&from, "in-process")?;
        let rx = self.sender(topic).subscribe();
        let stream =
            tokio_stream::wrappers::BroadcastStream::new(rx).filter_map(std::result::Result::ok);
        Ok(Box::pin(stream))
    }
}

impl Adapter for InProcessEventBus {
    fn name(&self) -> &'static str {
        "InProcessEventBus"
    }

    fn scope(&self) -> Scope {
        Scope::Local
    }

    fn remedy(&self) -> Option<&'static str> {
        Some("set EVENT_BUS to a shared adapter (postgres), or run one replica")
    }
}
