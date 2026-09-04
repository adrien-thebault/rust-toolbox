//! Server-sent events.
//!
//! # Why SSE rather than WebSockets
//!
//! Most of what people want realtime for is one-directional, and for that SSE
//! wins on every axis that matters: it is plain HTTP so it crosses every proxy,
//! reconnection with `Last-Event-ID` replay is built into the browser, HTTP/2
//! multiplexing is free, and you can `curl` it.
//!
//! Reach for WebSockets only when the client genuinely needs to **send** on the
//! same connection. Routing writes through a socket gives up validation,
//! OpenAPI, rate limiting, idempotency and normal error handling, all of which
//! a `POST` gets for free.
//!
//! # And no subscribe protocol
//!
//! One stream per topic, no envelope, no subscribe frame: HTTP/2 multiplexing
//! removes the per-origin connection limit that a multiplexing protocol would
//! otherwise be working around.
//!
//! # What a client must do
//!
//! Not packaged, since the client half is out of scope here. The short
//! version: fetch a ticket, open the stream, resume from the last id seen,
//! back off with jitter, and show a `live | reconnecting` state so the UI can
//! admit it is stale rather than lying.

pub mod hub;
pub mod sse;
pub mod ticket;

pub use hub::{Hub, HubConfig, SlowConsumer};
pub use sse::{LAST_EVENT_ID, SseConfig, resume_from, sse_from_events};
pub use ticket::{TICKET_TTL, Ticket, TicketStore};
