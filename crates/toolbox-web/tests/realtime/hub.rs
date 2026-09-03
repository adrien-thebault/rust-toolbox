use toolbox_web::realtime::hub::{Hub, HubConfig, SlowConsumer};

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
