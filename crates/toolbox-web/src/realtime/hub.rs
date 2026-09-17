//! One upstream subscription per topic, fanned out to every connection.
//!
//! The naive implementation opens one upstream subscription per browser
//! connection. Five admin tabs is five; five hundred users is an outage, and it
//! looks fine in development where there is one.

use std::{
    collections::HashMap,
    fmt,
    sync::{Mutex, PoisonError},
};

use futures_core::Stream;
use futures_util::stream;
use tokio::sync::broadcast::{self, error::RecvError};
use tracing::warn;

/// Fans one upstream stream per topic out to many connections.
pub struct Hub<T> {
    /// One fan-out sender per topic, created on first subscribe.
    topics: Mutex<HashMap<String, broadcast::Sender<T>>>,
    /// Per-topic channel capacity.
    buffer: usize,
}

impl<T> fmt::Debug for Hub<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Hub")
            .field("buffer", &self.buffer)
            .finish_non_exhaustive()
    }
}

impl<T: Clone + Send + 'static> Hub<T> {
    /// A hub with the given behaviour.
    ///
    /// # Arguments
    ///
    /// * `buffer` - Messages retained for a slow subscriber. Clamped to one.
    #[must_use]
    pub fn new(buffer: usize) -> Self {
        Self {
            topics: Mutex::new(HashMap::new()),
            buffer: buffer.max(1),
        }
    }

    /// Subscribe a connection to a topic.
    ///
    /// The first subscriber to a topic creates its channel; later ones share
    /// it, which is the whole point.
    ///
    /// # Arguments
    ///
    /// * `topic` - What to attach to. The first subscriber creates the
    ///   upstream; every later one shares it, which is the whole point of the
    ///   hub.
    pub fn subscribe(&self, topic: &str) -> broadcast::Receiver<T> {
        self.sender(topic).subscribe()
    }

    /// Subscribe as a stream suitable for a long-lived response.
    ///
    /// A subscriber that falls behind is closed instead of silently skipping
    /// messages. The browser then reconnects and re-queries the authoritative
    /// resource, which is the only way an ephemeral hub can recover safely.
    pub fn stream(&self, topic: &str) -> impl Stream<Item = T> + Send + 'static + use<T> {
        let receiver = self.subscribe(topic);
        stream::unfold(receiver, |mut receiver| async move {
            match receiver.recv().await {
                Ok(message) => Some((message, receiver)),
                Err(RecvError::Lagged(missed)) => {
                    warn!(missed, "closing a hub subscriber that fell behind");
                    None
                }
                Err(RecvError::Closed) => None,
            }
        })
    }

    /// Whether a topic already has an upstream.
    ///
    /// # Arguments
    ///
    /// * `topic` - The topic to test, so a caller can decide whether it needs
    ///   to start an upstream.
    #[must_use]
    pub fn has_topic(&self, topic: &str) -> bool {
        self.topics
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .contains_key(topic)
    }

    /// How many connections are attached to a topic.
    ///
    /// # Arguments
    ///
    /// * `topic` - The topic to count. Zero is normal, and is what the idle
    ///   timeout eventually collects.
    #[must_use]
    pub fn subscribers(&self, topic: &str) -> usize {
        self.topics
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get(topic)
            .map_or(0, broadcast::Sender::receiver_count)
    }

    /// How many topics have an upstream.
    #[must_use]
    pub fn topic_count(&self) -> usize {
        self.topics
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .len()
    }

    /// Push a message to every connection on a topic.
    ///
    /// Returns how many received it. Zero is not an error: a topic with no
    /// subscribers is normal.
    ///
    /// # Arguments
    ///
    /// * `topic` - Where to send it.
    /// * `message` - What to send. Every attached connection gets a clone.
    pub fn publish(&self, topic: &str, message: T) -> usize {
        self.sender(topic).send(message).unwrap_or(0)
    }

    /// Drop the upstream for topics nobody is listening to.
    ///
    /// Without this a hub accumulates one channel per topic ever seen, which
    /// for a topic-per-entity scheme is unbounded.
    pub fn prune(&self) -> usize {
        let mut topics = self.topics.lock().unwrap_or_else(PoisonError::into_inner);
        let before = topics.len();
        topics.retain(|_, sender| sender.receiver_count() > 0);
        before - topics.len()
    }

    /// The broadcast channel for a topic, created on first use.
    ///
    /// # Arguments
    ///
    /// * `topic` - The topic whose channel is wanted.
    fn sender(&self, topic: &str) -> broadcast::Sender<T> {
        let mut topics = self.topics.lock().unwrap_or_else(PoisonError::into_inner);
        topics
            .entry(topic.to_owned())
            .or_insert_with(|| broadcast::channel(self.buffer).0)
            .clone()
    }
}
