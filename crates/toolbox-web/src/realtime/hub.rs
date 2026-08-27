//! One upstream subscription per topic, fanned out to every connection.
//!
//! The naive implementation opens one upstream subscription per browser
//! connection. Five admin tabs is five; five hundred users is an outage, and it
//! looks fine in development where there is one.

use std::{collections::HashMap, sync::Mutex, time::Duration};

use tokio::sync::broadcast;

/// What to do when a connection stops reading.
///
/// **No default.** A browser on a train stops reading, and without a bounded
/// buffer the gateway grows one until it dies. Which of these is right depends
/// on the stream - dropping frames of a live feed is fine, dropping an audit
/// event is not - so there is nothing sensible to guess.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SlowConsumer {
    /// Discard the oldest buffered messages and keep the connection.
    DropOldest,
    /// Close the connection and let the client reconnect and resume.
    Close,
}

/// How a hub behaves.
#[derive(Debug, Clone, Copy)]
pub struct HubConfig {
    /// How many messages a connection may fall behind by.
    pub buffer: usize,
    /// What to do when it falls further.
    pub slow_consumer: SlowConsumer,
    /// How long a topic with no subscribers is kept before its upstream
    /// subscription is dropped.
    pub idle_timeout: Duration,
}

impl HubConfig {
    /// A config. There is no `Default`, because [`SlowConsumer`] has no
    /// defensible default.
    ///
    /// # Arguments
    ///
    /// * `buffer` - How many messages a connection may fall behind before the
    ///   slow-consumer policy applies.
    /// * `slow_consumer` - What to do when it does. There is no default,
    ///   because a browser on a train stops reading and the right answer
    ///   depends on the stream.
    #[must_use]
    pub fn new(buffer: usize, slow_consumer: SlowConsumer) -> Self {
        Self {
            buffer,
            slow_consumer,
            idle_timeout: Duration::from_secs(60),
        }
    }

    /// How long an unsubscribed topic is kept.
    ///
    /// # Arguments
    ///
    /// * `timeout` - How long a topic with no subscribers keeps its upstream,
    ///   so a reconnecting client does not pay to re-establish it.
    #[must_use]
    pub fn idle_timeout(mut self, timeout: Duration) -> Self {
        self.idle_timeout = timeout;
        self
    }
}

/// Fans one upstream stream per topic out to many connections.
pub struct Hub<T> {
    topics: Mutex<HashMap<String, broadcast::Sender<T>>>,
    config: HubConfig,
}

impl<T> std::fmt::Debug for Hub<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Hub")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl<T: Clone + Send + 'static> Hub<T> {
    /// A hub with the given behaviour.
    ///
    /// # Arguments
    ///
    /// * `config` - Buffer size, slow-consumer policy and idle timeout.
    #[must_use]
    pub fn new(config: HubConfig) -> Self {
        Self {
            topics: Mutex::new(HashMap::new()),
            config,
        }
    }

    /// How this hub is configured.
    #[must_use]
    pub fn config(&self) -> HubConfig {
        self.config
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
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .get(topic)
            .map_or(0, tokio::sync::broadcast::Sender::receiver_count)
    }

    /// How many topics have an upstream.
    #[must_use]
    pub fn topic_count(&self) -> usize {
        self.topics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
        let mut topics = self
            .topics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
        let mut topics = self
            .topics
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        topics
            .entry(topic.to_owned())
            .or_insert_with(|| broadcast::channel(self.config.buffer).0)
            .clone()
    }
}
