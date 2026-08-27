//! The default bus: a tokio broadcast channel per topic.

use std::{collections::HashMap, sync::Mutex};

use async_trait::async_trait;
use tokio_stream::StreamExt as _;

use super::{
    BusCapabilities, BusError, BusOrdering, Delivery, EventBus, EventStream, MissingCapability,
    StartPosition, Topic,
};
use crate::{
    deployment::{Adapter, Scope},
    event::CloudEvent,
};

/// The default bus: a tokio broadcast channel per topic.
///
/// **Single replica only.** Events published on this instance never reach a
/// subscriber on another, so under `DEPLOYMENT=clustered` a subscriber misses
/// most of the stream. That is why it declares [`Scope::Local`] and the
/// startup guard refuses to run it clustered.
pub struct InProcessBus {
    topics: Mutex<HashMap<Topic, tokio::sync::broadcast::Sender<CloudEvent>>>,
    buffer: usize,
}

impl std::fmt::Debug for InProcessBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InProcessBus")
            .field("buffer", &self.buffer)
            .finish_non_exhaustive()
    }
}

impl Default for InProcessBus {
    fn default() -> Self {
        Self::new(1024)
    }
}

impl InProcessBus {
    /// A bus buffering `buffer` events per topic for slow subscribers.
    ///
    /// # Arguments
    ///
    /// * `buffer` - How many events to hold per topic for a subscriber that has
    ///   fallen behind. Past it, the slow subscriber loses events rather than
    ///   the publisher blocking.
    #[must_use]
    pub fn new(buffer: usize) -> Self {
        Self {
            topics: Mutex::new(HashMap::new()),
            buffer,
        }
    }

    /// The broadcast channel for a topic, created on first use.
    ///
    /// # Arguments
    ///
    /// * `topic` - The topic whose channel is wanted.
    fn sender(&self, topic: &Topic) -> tokio::sync::broadcast::Sender<CloudEvent> {
        let mut topics = self
            .topics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        topics
            .entry(topic.clone())
            .or_insert_with(|| tokio::sync::broadcast::channel(self.buffer).0)
            .clone()
    }
}

#[async_trait]
impl EventBus for InProcessBus {
    fn capabilities(&self) -> BusCapabilities {
        BusCapabilities {
            delivery: Delivery::AtMostOnce,
            replay: None,
            ordering: BusOrdering::PerTopic,
            max_payload: usize::MAX,
            durable: false,
        }
    }

    async fn publish(&self, topic: &Topic, event: CloudEvent) -> Result<(), BusError> {
        // An error here means nobody is subscribed, which is not a failure.
        let _ = self.sender(topic).send(event);
        Ok(())
    }

    async fn subscribe(&self, topic: &Topic, from: StartPosition) -> Result<EventStream, BusError> {
        if matches!(from, StartPosition::Cursor(_)) {
            return Err(BusError::Unsupported {
                needed: MissingCapability::Replay,
                adapter: "in-process",
            });
        }
        let rx = self.sender(topic).subscribe();
        let stream =
            tokio_stream::wrappers::BroadcastStream::new(rx).filter_map(std::result::Result::ok);
        Ok(Box::pin(stream))
    }
}

impl Adapter for InProcessBus {
    fn name(&self) -> &'static str {
        "InProcessBus"
    }

    fn scope(&self) -> Scope {
        Scope::Local
    }

    fn remedy(&self) -> Option<&'static str> {
        Some("set EVENT_BUS to a shared adapter (postgres), or run one replica")
    }
}
