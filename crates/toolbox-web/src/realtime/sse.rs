//! Turning a stream of events into the SSE wire format.

use std::{convert::Infallible, time::Duration};

use axum::response::sse::{Event, KeepAlive, Sse};
use cloudevents::AttributesReader as _;
use futures_core::Stream;
use futures_util::StreamExt as _;
use toolbox_cluster::CloudEvent;

/// How a stream behaves.
#[derive(Debug, Clone, Copy)]
pub struct SseConfig {
    /// How often to send a comment when nothing else is flowing.
    ///
    /// Every proxy between you and the browser has an idle timeout, and a
    /// stream that says nothing for long enough is closed by one of them. This
    /// must be shorter than the shortest of those.
    pub keep_alive: Duration,
}

impl Default for SseConfig {
    fn default() -> Self {
        Self {
            keep_alive: Duration::from_secs(15),
        }
    }
}

/// Turn a stream of events into an SSE response.
///
/// Every event carries its `CloudEvent` id as the SSE id, which is what the
/// browser sends back as `Last-Event-ID` on reconnect. Without that a
/// reconnection silently loses whatever arrived while it was gone, and the
/// table on screen is quietly wrong.
///
/// # Arguments
///
/// * `events` - The events to stream. Each one's `CloudEvent` id becomes the SSE
///   id, which is what the browser sends back on reconnect.
/// * `cfg` - Heartbeat interval and retry hint. A stream without a heartbeat
///   dies silently behind a proxy.
pub fn sse_from_events<S>(
    events: S,
    cfg: SseConfig,
) -> Sse<impl Stream<Item = Result<Event, Infallible>> + Send>
where
    S: Stream<Item = CloudEvent> + Send + 'static,
{
    let stream = events.map(|event| {
        let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_owned());
        Ok(Event::default().id(event.id()).event(event.ty()).data(data))
    });

    Sse::new(stream).keep_alive(KeepAlive::new().interval(cfg.keep_alive).text("keep-alive"))
}

/// The header a browser sends when resuming a stream.
pub const LAST_EVENT_ID: &str = "last-event-id";

/// Where a reconnecting client wants to resume from.
///
/// A browser sends `Last-Event-ID`; a non-browser client that cannot may pass
/// `?last_event_id=`, so both work.
///
/// # Arguments
///
/// * `headers` - The request headers, read for `Last-Event-ID`, which is what a
///   browser sends.
/// * `query` - The query string, read for `last_event_id` so a non-browser
///   client that cannot set the header still resumes.
#[must_use]
pub fn resume_from(headers: &http::HeaderMap, query: Option<&str>) -> Option<String> {
    headers
        .get(LAST_EVENT_ID)
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .or_else(|| query.map(str::to_owned))
        .filter(|id| !id.is_empty())
}
