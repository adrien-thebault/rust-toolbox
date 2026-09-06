use axum::{Router, routing::get};
use futures_util::stream;
use toolbox_cluster::event;
use toolbox_web::realtime::{SseConfig, resume_from, sse_from_events};

use crate::{call, get as get_req};

/// The stream opens with a comment - so a buffering proxy flushes the response
/// headers at once instead of holding them until the first real event - then
/// emits one frame per event, each carrying its CloudEvents `type` and a JSON
/// `data` payload.
#[tokio::test]
async fn the_stream_opens_with_a_comment_then_one_frame_per_event() {
    let app = Router::new().route(
        "/stream",
        get(|| async {
            let events = stream::iter(vec![
                event("todo.created", "test/1", &serde_json::json!({ "n": 1 })).unwrap(),
                event("todo.done", "test/1", &serde_json::json!({ "n": 1 })).unwrap(),
            ]);
            sse_from_events(events, SseConfig::default())
        }),
    );

    let (res, body) = call(app, get_req("/stream")).await;
    assert_eq!(res.headers()["content-type"], "text/event-stream");
    assert!(body.contains("connected"), "opening comment: {body:?}");
    assert!(body.contains("todo.created"), "first event type: {body:?}");
    assert!(body.contains("todo.done"), "second event type: {body:?}");
    assert!(
        body.contains(r#""n":1"#),
        "the payload is serialised as data: {body:?}"
    );
}

#[test]
fn the_keep_alive_interval_is_configurable_and_defaults_to_fifteen_seconds() {
    assert_eq!(
        SseConfig::default().keep_alive,
        std::time::Duration::from_secs(15)
    );
}

/// Without resume, a reconnection silently loses whatever arrived while it was
/// gone, and the table on screen is quietly wrong.
#[test]
fn a_browser_resumes_with_the_last_event_id_header() {
    let mut headers = http::HeaderMap::new();
    headers.insert("last-event-id", "evt-42".parse().unwrap());
    assert_eq!(resume_from(&headers, None), Some("evt-42".to_owned()));
}

#[test]
fn a_non_browser_client_may_resume_with_a_query_parameter() {
    assert_eq!(
        resume_from(&http::HeaderMap::new(), Some("evt-42")),
        Some("evt-42".to_owned())
    );
}

#[test]
fn the_header_wins_over_the_query_parameter() {
    let mut headers = http::HeaderMap::new();
    headers.insert("last-event-id", "from-header".parse().unwrap());
    assert_eq!(
        resume_from(&headers, Some("from-query")),
        Some("from-header".to_owned())
    );
}

#[test]
fn a_fresh_connection_resumes_from_nothing() {
    assert_eq!(resume_from(&http::HeaderMap::new(), None), None);
    assert_eq!(resume_from(&http::HeaderMap::new(), Some("")), None);
}
