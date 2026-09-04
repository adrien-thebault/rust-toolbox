use toolbox_web::realtime::resume_from;

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
