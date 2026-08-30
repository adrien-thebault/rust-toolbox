use std::{sync::Arc, time::Duration};

use toolbox_auth::Principal;
use toolbox_cluster::InMemoryKvStore;
use toolbox_web::realtime::{
    Tickets,
    hub::{Hub, HubConfig, SlowConsumer},
    resume_from,
};

fn tickets() -> Tickets {
    Tickets::new(Arc::new(InMemoryKvStore::default())).unwrap()
}

fn principal() -> Principal {
    Principal::new("ada", "password").with_role("ADMIN")
}

// --- Tickets ------------------------------------------------------------

#[tokio::test]
async fn a_ticket_round_trips_for_its_own_topic() {
    let tickets = tickets();
    let ticket = tickets.issue(&principal(), "orders").await.unwrap();
    let redeemed = tickets.redeem(&ticket, "orders").await.unwrap();
    assert_eq!(redeemed, principal());
}

/// If a ticket leaks into a log it must already be worthless.
#[tokio::test]
async fn a_ticket_is_single_use() {
    let tickets = tickets();
    let ticket = tickets.issue(&principal(), "orders").await.unwrap();

    tickets.redeem(&ticket, "orders").await.unwrap();
    let err = tickets.redeem(&ticket, "orders").await.unwrap_err();
    assert_eq!(err.status(), http::StatusCode::UNAUTHORIZED);
    assert_eq!(err.problem().code.as_deref(), Some("INVALID_TICKET"));
}

/// The authorization that cannot be skipped: the ticket names its topic.
#[tokio::test]
async fn a_ticket_for_one_topic_cannot_open_another() {
    let tickets = tickets();
    let ticket = tickets.issue(&principal(), "orders").await.unwrap();

    let err = tickets.redeem(&ticket, "payroll").await.unwrap_err();
    assert_eq!(err.status(), http::StatusCode::FORBIDDEN);
    assert_eq!(err.problem().code.as_deref(), Some("TICKET_TOPIC_MISMATCH"));
}

#[tokio::test]
async fn an_unknown_ticket_is_refused() {
    let err = tickets()
        .redeem("not-a-ticket", "orders")
        .await
        .unwrap_err();
    assert_eq!(err.status(), http::StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn an_expired_ticket_is_refused() {
    let tickets = tickets().ttl(Duration::from_millis(20));
    let ticket = tickets.issue(&principal(), "orders").await.unwrap();

    tokio::time::sleep(Duration::from_millis(80)).await;
    assert!(tickets.redeem(&ticket, "orders").await.is_err());
}

#[tokio::test]
async fn a_ticket_carries_nothing_readable() {
    let ticket = tickets().issue(&principal(), "orders").await.unwrap();
    assert_eq!(ticket.len(), 64, "256 bits, hex");
    assert!(ticket.chars().all(|c| c.is_ascii_hexdigit()));
    assert!(!ticket.contains("ada") && !ticket.contains("orders"));
}

/// Without an atomic take a ticket is not single-use, and two connections
/// could redeem the same one.
#[tokio::test]
async fn tickets_refuse_a_store_that_cannot_take_atomically() {
    struct NoTake;

    #[async_trait::async_trait]
    impl toolbox_cluster::KvStore for NoTake {
        fn capabilities(&self) -> toolbox_cluster::KvStoreCapabilities {
            toolbox_cluster::KvStoreCapabilities {
                atomic_take: false,
                atomic_add: false,
                ttl: true,
                durable: false,
                shared: false,
            }
        }
        async fn get(&self, _k: &str) -> Result<Option<Vec<u8>>, toolbox_cluster::KvStoreError> {
            Ok(None)
        }
        async fn set(
            &self,
            _k: &str,
            _v: Vec<u8>,
            _t: Option<Duration>,
        ) -> Result<(), toolbox_cluster::KvStoreError> {
            Ok(())
        }
        async fn add(
            &self,
            _k: &str,
            _v: Vec<u8>,
            _t: Option<Duration>,
        ) -> Result<bool, toolbox_cluster::KvStoreError> {
            Ok(true)
        }
        async fn take(&self, _k: &str) -> Result<Option<Vec<u8>>, toolbox_cluster::KvStoreError> {
            Ok(None)
        }
        async fn delete(&self, _k: &str) -> Result<(), toolbox_cluster::KvStoreError> {
            Ok(())
        }
    }

    assert!(Tickets::new(Arc::new(NoTake)).is_err());
}

// --- Hub ----------------------------------------------------------------

fn hub() -> Hub<String> {
    Hub::new(HubConfig::new(16, SlowConsumer::DropOldest))
}

/// The naive implementation opens one upstream per browser connection: five
/// admin tabs is five, five hundred users is an outage.
#[tokio::test]
async fn many_connections_to_one_topic_share_one_upstream() {
    let hub = hub();
    let _a = hub.subscribe("orders");
    let _b = hub.subscribe("orders");
    let _c = hub.subscribe("orders");

    assert_eq!(hub.topic_count(), 1, "one upstream, not three");
    assert_eq!(hub.subscribers("orders"), 3);
}

#[tokio::test]
async fn every_connection_on_a_topic_receives_a_message() {
    let hub = hub();
    let mut a = hub.subscribe("orders");
    let mut b = hub.subscribe("orders");

    assert_eq!(hub.publish("orders", "hello".to_owned()), 2);
    assert_eq!(a.recv().await.unwrap(), "hello");
    assert_eq!(b.recv().await.unwrap(), "hello");
}

#[tokio::test]
async fn a_message_does_not_cross_topics() {
    let hub = hub();
    let mut orders = hub.subscribe("orders");
    hub.publish("payroll", "secret".to_owned());

    assert!(
        orders.try_recv().is_err(),
        "nothing arrived on the other topic"
    );
}

#[tokio::test]
async fn publishing_with_nobody_listening_is_not_an_error() {
    assert_eq!(hub().publish("orders", "x".to_owned()), 0);
}

/// A browser on a train stops reading; without a bound the gateway grows a
/// buffer until it dies.
#[tokio::test]
async fn a_slow_consumer_is_bounded_rather_than_unbounded() {
    let hub = Hub::<u32>::new(HubConfig::new(4, SlowConsumer::DropOldest));
    let mut slow = hub.subscribe("orders");

    for i in 0..100 {
        hub.publish("orders", i);
    }

    // The channel is bounded, so the reader is told it lagged rather than the
    // buffer growing to a hundred.
    match slow.try_recv() {
        Err(tokio::sync::broadcast::error::TryRecvError::Lagged(n)) => {
            assert!(n > 0, "the connection was told how far behind it fell");
        }
        other => panic!("expected a lag notification, got {other:?}"),
    }
}

/// A hub accumulates one channel per topic ever seen, which for a
/// topic-per-entity scheme is unbounded.
#[tokio::test]
async fn topics_nobody_listens_to_are_pruned() {
    let hub = hub();
    {
        let _a = hub.subscribe("orders");
        let _b = hub.subscribe("payroll");
        assert_eq!(hub.topic_count(), 2);
        assert_eq!(hub.prune(), 0, "nothing to prune while both are subscribed");
    }
    assert_eq!(hub.prune(), 2, "both were dropped");
    assert_eq!(hub.topic_count(), 0);
}

/// `SlowConsumer` has no default because neither answer is right for every
/// stream, and guessing is how you drop an audit event.
#[test]
fn a_hub_config_must_state_its_slow_consumer_policy() {
    let cfg = HubConfig::new(64, SlowConsumer::Close);
    assert_eq!(cfg.slow_consumer, SlowConsumer::Close);
    assert_eq!(cfg.buffer, 64);
}

// --- Resume -------------------------------------------------------------

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
