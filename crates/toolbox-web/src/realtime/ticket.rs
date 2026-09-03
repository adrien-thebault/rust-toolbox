//! Single-use tickets for stream authentication.
//!
//! Browsers cannot set headers on an `EventSource` or a WebSocket handshake, so
//! the usual workaround puts the session token in the query string - where it
//! lands in every access log, every proxy log and the browser's history, valid
//! for hours.
//!
//! A ticket is single-use, short-lived, topic-scoped and principal-bound. If
//! one leaks into a log it is already expired and already consumed. And because
//! the ticket names the topic it was issued for, it is **topic authorization
//! that cannot be skipped**: a ticket for one stream cannot open another.

use std::{sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use toolbox_auth::Principal;
use toolbox_cluster::KvStore;

use crate::error::ApiError;

/// How long a ticket is valid.
///
/// Thirty seconds: long enough for a browser to follow up, short enough that a
/// leaked one is worthless.
pub const TICKET_TTL: Duration = Duration::from_secs(30);

/// The key prefix in the shared store.
const PREFIX: &str = "toolbox:ticket:";

/// What a redeemed ticket proves.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ticket {
    /// Who asked for it.
    pub principal: Principal,
    /// The topic it is good for, and no other.
    pub topic: String,
}

/// Issues and redeems stream tickets.
pub struct TicketStore {
    /// Where single-use tickets are stored.
    kv: Arc<dyn KvStore>,
    /// How long an unredeemed ticket lives.
    ttl: Duration,
}

impl std::fmt::Debug for TicketStore {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TicketStore")
            .field("ttl", &self.ttl)
            .finish_non_exhaustive()
    }
}

impl TicketStore {
    /// Build over a key-value store.
    ///
    /// # Arguments
    ///
    /// * `kv` - The store. Without an atomic take a ticket is not single-use,
    ///   and two connections could redeem the same one.
    ///
    /// # Errors
    /// [`ApiError`] when the adapter cannot promise an atomic take - without
    /// it a ticket is not single-use, and two connections could redeem the
    /// same one.
    pub fn new(kv: Arc<dyn KvStore>) -> Result<Self, ApiError> {
        if !kv.capabilities().atomic_take {
            return Err(ApiError::internal(std::io::Error::other(
                "realtime tickets need a key-value store with an atomic take",
            )));
        }
        Ok(Self {
            kv,
            ttl: TICKET_TTL,
        })
    }

    /// Override the lifetime.
    ///
    /// # Arguments
    ///
    /// * `ttl` - How long a ticket stays valid. Long enough for the browser to
    ///   follow up, short enough that a leaked one is worthless.
    #[must_use]
    pub fn ttl(mut self, ttl: Duration) -> Self {
        self.ttl = ttl;
        self
    }

    /// Issue a ticket for one principal and one topic.
    ///
    /// # Arguments
    ///
    /// * `principal` - Who the ticket will authenticate.
    /// * `topic` - The single topic it is good for. Binding it here is what
    ///   stops a ticket for one stream opening another.
    ///
    /// # Errors
    /// [`ApiError`] when the store fails.
    pub async fn issue(&self, principal: &Principal, topic: &str) -> Result<String, ApiError> {
        let token = new_token();
        let ticket = Ticket {
            principal: principal.clone(),
            topic: topic.to_owned(),
        };
        let value = serde_json::to_vec(&ticket).map_err(ApiError::internal)?;

        self.kv
            .set(&key(&token), value, Some(self.ttl))
            .await
            .map_err(ApiError::internal)?;
        Ok(token)
    }

    /// Redeem a ticket for a topic, consuming it.
    ///
    /// # Arguments
    ///
    /// * `token` - The opaque ticket from the query string. It is consumed, so
    ///   a replay fails.
    /// * `topic` - The topic being opened, checked against the one the ticket
    ///   was issued for.
    ///
    /// # Errors
    /// [`ApiError`] 401 when the ticket is unknown, expired or already used;
    /// 403 when it was issued for a different topic.
    pub async fn redeem(&self, token: &str, topic: &str) -> Result<Principal, ApiError> {
        let raw = self
            .kv
            .take(&key(token))
            .await
            .map_err(ApiError::internal)?
            .ok_or_else(|| {
                ApiError::unauthenticated()
                    .with_code("INVALID_TICKET")
                    .with_detail("the ticket is unknown, expired or already used")
            })?;

        let ticket: Ticket = serde_json::from_slice(&raw).map_err(ApiError::internal)?;

        // The ticket names its topic, so a ticket for one stream cannot open
        // another. This is the authorization that cannot be skipped.
        if ticket.topic != topic {
            return Err(ApiError::forbidden("this ticket is for a different topic")
                .with_code("TICKET_TOPIC_MISMATCH"));
        }
        Ok(ticket.principal)
    }
}

/// The store key for a ticket, prefixed so tickets cannot collide with anything
/// else in a shared store.
///
/// # Arguments
///
/// * `token` - The opaque ticket.
fn key(token: &str) -> String {
    format!("{PREFIX}{token}")
}

/// A 256-bit opaque token, hex-encoded.
///
/// Straight from the OS source, not from a UUID: a ticket is a secret looked
/// up by exact key, so it wants entropy and nothing else. A v7 would spend a
/// third of its bits on a timestamp the holder already knows.
fn new_token() -> String {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes).expect("the OS random source");
    bytes.iter().fold(String::with_capacity(64), |mut out, b| {
        use std::fmt::Write as _;
        let _ = write!(out, "{b:02x}");
        out
    })
}
