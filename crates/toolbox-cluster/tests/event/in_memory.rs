use std::time::Duration;

use cloudevents::AttributesReader as _;
use futures_util::StreamExt;
use toolbox_cluster::{EventBus, InMemoryEventBus, Topic, signal};

#[tokio::test]
async fn a_published_event_reaches_a_subscriber() {
    let bus = InMemoryEventBus::default();
    let topic = Topic::new("things");
    let mut stream = bus.subscribe(&topic).await.unwrap();

    bus.publish(&topic, signal("created", "/things").unwrap())
        .await
        .unwrap();

    let got = tokio::time::timeout(Duration::from_secs(1), stream.next())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(got.ty(), "created");
}

#[tokio::test]
async fn every_subscriber_of_a_topic_sees_the_event() {
    let bus = InMemoryEventBus::default();
    let topic = Topic::new("things");
    let mut a = bus.subscribe(&topic).await.unwrap();
    let mut b = bus.subscribe(&topic).await.unwrap();

    bus.publish(&topic, signal("created", "/things").unwrap())
        .await
        .unwrap();

    assert!(
        tokio::time::timeout(Duration::from_secs(1), a.next())
            .await
            .unwrap()
            .is_some()
    );
    assert!(
        tokio::time::timeout(Duration::from_secs(1), b.next())
            .await
            .unwrap()
            .is_some()
    );
}

#[tokio::test]
async fn a_subscriber_does_not_see_another_topic() {
    let bus = InMemoryEventBus::default();
    let mut stream = bus.subscribe(&Topic::new("a")).await.unwrap();
    bus.publish(&Topic::new("b"), signal("x", "/y").unwrap())
        .await
        .unwrap();

    let got = tokio::time::timeout(Duration::from_millis(100), stream.next()).await;
    assert!(got.is_err(), "nothing arrived, which is what should happen");
}

#[tokio::test]
async fn publishing_with_nobody_listening_is_not_an_error() {
    let bus = InMemoryEventBus::default();
    assert!(
        bus.publish(&Topic::new("a"), signal("x", "/y").unwrap())
            .await
            .is_ok()
    );
}

#[tokio::test]
async fn a_zero_buffer_is_clamped_rather_than_panicking() {
    let bus = InMemoryEventBus::new(0);
    let topic = Topic::new("a");
    let mut stream = bus.subscribe(&topic).await.unwrap();
    bus.publish(&topic, signal("e", "/e").unwrap())
        .await
        .unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(1), stream.next())
            .await
            .unwrap()
            .is_some()
    );
}
