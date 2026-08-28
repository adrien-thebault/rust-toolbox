//! The transactional outbox.
//!
//! It encodes the decision that an event is written in the **same transaction**
//! as the change that caused it. Publishing directly from a handler means an
//! event for a change that rolled back, or a change with no event, depending on
//! which side failed - and neither is visible until it matters.
//!
//! It is also what makes a later broker swap a config change rather than an
//! audit of every publish site: delivery here is already at-least-once, so
//! nothing downstream assumes exactly-once.

use std::time::Duration;

use async_trait::async_trait;
use diesel::{pg::PgConnection, prelude::*};
use toolbox_cluster::{
    CloudEvent,
    bus::{
        BusCapabilities, BusError, BusOrdering, Delivery, EventBus, EventStream, MissingCapability,
        StartPosition, Topic,
    },
    deployment::{Adapter, Scope},
};
use toolbox_db::Db;

use crate::schema::toolbox_outbox;

/// How often the relay looks for unpublished rows.
const DEFAULT_POLL: Duration = Duration::from_millis(500);

/// How many rows the relay claims at once.
const BATCH: i64 = 100;

/// An event bus backed by an outbox table.
#[derive(Clone)]
pub struct OutboxBus {
    db: Db<PgConnection>,
    poll: Duration,
}

impl std::fmt::Debug for OutboxBus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OutboxBus")
            .field("poll", &self.poll)
            .finish_non_exhaustive()
    }
}

impl OutboxBus {
    /// Build over a pool.
    ///
    /// # Arguments
    ///
    /// * `db` - The pool holding the outbox table.
    #[must_use]
    pub fn new(db: Db<PgConnection>) -> Self {
        Self {
            db,
            poll: DEFAULT_POLL,
        }
    }

    /// How often the relay polls.
    ///
    /// # Arguments
    ///
    /// * `poll` - How often the relay looks for unpublished rows. It is the
    ///   floor on delivery latency, and the rate of an idle query.
    #[must_use]
    pub fn poll_interval(mut self, poll: Duration) -> Self {
        self.poll = poll;
        self
    }

    /// Write an event inside a transaction the caller already owns.
    ///
    /// **This is the method that matters.** Call it from inside
    /// `Db::transaction`, alongside the domain change, and the event and the
    /// change commit or roll back together.
    ///
    /// # Arguments
    ///
    /// * `conn` - A connection already inside the caller's transaction. That is
    ///   the whole point: the event commits with the domain change or not at
    ///   all.
    /// * `topic` - Where the relay will publish it.
    /// * `event` - The event to store.
    ///
    /// # Errors
    /// [`diesel::result::Error`] when the insert fails, so it aborts the
    /// caller's transaction like any other statement.
    pub fn enqueue(
        conn: &mut PgConnection,
        topic: &Topic,
        event: &CloudEvent,
    ) -> Result<(), diesel::result::Error> {
        let payload = serde_json::to_value(event)
            .map_err(|e| diesel::result::Error::SerializationError(Box::new(e)))?;

        diesel::insert_into(toolbox_outbox::table)
            .values((
                toolbox_outbox::topic.eq(topic.as_str()),
                toolbox_outbox::event.eq(payload),
            ))
            .execute(conn)
            .map(|_| ())
    }

    /// Claim a batch of unpublished events and mark them published.
    ///
    /// `FOR UPDATE SKIP LOCKED` is what lets several replicas run the relay at
    /// once: each claims rows the others are not holding, rather than all of
    /// them contending on the same head of the queue.
    ///
    /// # Errors
    /// [`BusError::Transport`] when the statement fails.
    pub async fn drain_batch(&self) -> Result<Vec<(Topic, CloudEvent)>, BusError> {
        let rows = self
            .db
            .query(|c: &mut PgConnection| {
                c.transaction(|c| {
                    let claimed: Vec<(i64, String, serde_json::Value)> = toolbox_outbox::table
                        .filter(toolbox_outbox::published_at.is_null())
                        .order(toolbox_outbox::id.asc())
                        .limit(BATCH)
                        .select((
                            toolbox_outbox::id,
                            toolbox_outbox::topic,
                            toolbox_outbox::event,
                        ))
                        .for_update()
                        .skip_locked()
                        .load(c)?;

                    if claimed.is_empty() {
                        return Ok(claimed);
                    }
                    let ids: Vec<i64> = claimed.iter().map(|(id, _, _)| *id).collect();
                    diesel::update(toolbox_outbox::table.filter(toolbox_outbox::id.eq_any(ids)))
                        .set(toolbox_outbox::published_at.eq(chrono::Utc::now()))
                        .execute(c)?;
                    Ok(claimed)
                })
            })
            .await
            .map_err(|e| BusError::Transport(e.to_string()))?;

        Ok(rows
            .into_iter()
            .filter_map(|(_, topic, payload)| {
                serde_json::from_value(payload)
                    .ok()
                    .map(|event: CloudEvent| (Topic::new(topic), event))
            })
            .collect())
    }

    /// Delete events published longer ago than `keep`.
    ///
    /// Without this the table is an append-only log of everything that ever
    /// happened, which is a fine thing to want and a terrible thing to get by
    /// accident.
    ///
    /// # Arguments
    ///
    /// * `keep` - How long a published row stays readable. Long enough to debug
    ///   an incident, short enough that the table is not an append-only log of
    ///   everything.
    ///
    /// # Errors
    /// [`BusError::Transport`] when the statement fails.
    pub async fn purge_published(&self, keep: Duration) -> Result<usize, BusError> {
        let cutoff = chrono::Utc::now()
            - chrono::Duration::from_std(keep).unwrap_or_else(|_| chrono::Duration::days(7));
        self.db
            .query(move |c: &mut PgConnection| {
                diesel::delete(
                    toolbox_outbox::table
                        .filter(toolbox_outbox::published_at.is_not_null())
                        .filter(toolbox_outbox::published_at.lt(cutoff)),
                )
                .execute(c)
            })
            .await
            .map_err(|e| BusError::Transport(e.to_string()))
    }

    /// How many events are waiting.
    ///
    /// The number to alert on: a backlog that only grows means the relay is
    /// not running.
    ///
    /// # Errors
    /// [`BusError::Transport`] when the statement fails.
    pub async fn backlog(&self) -> Result<i64, BusError> {
        self.db
            .query(|c: &mut PgConnection| {
                toolbox_outbox::table
                    .filter(toolbox_outbox::published_at.is_null())
                    .count()
                    .get_result(c)
            })
            .await
            .map_err(|e| BusError::Transport(e.to_string()))
    }
}

#[async_trait]
impl EventBus for OutboxBus {
    fn capabilities(&self) -> BusCapabilities {
        BusCapabilities {
            // At-least-once: a relay that publishes and then fails to mark the
            // row redelivers it. Handlers must be idempotent, and saying so
            // here is what stops somebody assuming otherwise.
            delivery: Delivery::AtLeastOnce,
            // Published rows stay until purged, so replay is possible for as
            // long as the retention allows.
            replay: Some(Duration::from_secs(7 * 24 * 60 * 60)),
            ordering: BusOrdering::PerTopic,
            // The practical ceiling on a JSONB column before it hurts.
            max_payload: 1024 * 1024,
            durable: true,
        }
    }

    async fn publish(&self, topic: &Topic, event: CloudEvent) -> Result<(), BusError> {
        let topic = topic.clone();
        self.db
            .query(move |c: &mut PgConnection| Self::enqueue(c, &topic, &event))
            .await
            .map_err(|e| BusError::Transport(e.to_string()))
    }

    async fn subscribe(
        &self,
        _topic: &Topic,
        from: StartPosition,
    ) -> Result<EventStream, BusError> {
        // Subscribing is the relay's job, not a caller's: the outbox is a
        // queue drained by whoever runs `drain_batch`, and handing out a
        // second consumer here would race with it.
        Err(BusError::Unsupported {
            needed: match from {
                StartPosition::Cursor(_) => MissingCapability::Replay,
                _ => MissingCapability::AtLeastOnce,
            },
            adapter: "outbox",
        })
    }
}

impl Adapter for OutboxBus {
    fn name(&self) -> &'static str {
        "OutboxBus"
    }

    fn scope(&self) -> Scope {
        Scope::Shared
    }
}
