use std::time::Duration;

use cloudevents::AttributesReader as _;
use futures_util::StreamExt;
use toolbox_cluster::{
    Delivery, EventBus, EventBusError, InProcessEventBus, MissingCapability, StartPosition, Topic,
    event, payload, signal,
};

#[test]
fn an_event_serializes_as_cloudevents_1_0() {
    let e = event(
        "com.example.thing.created",
        "/things",
        &serde_json::json!({"id": 7}),
    )
    .unwrap();
    let v: serde_json::Value = serde_json::to_value(&e).unwrap();

    assert_eq!(v["specversion"], "1.0");
    assert_eq!(
        v["type"], "com.example.thing.created",
        "the field is `type`, not `ty`"
    );
    assert_eq!(v["source"], "/things");
    assert_eq!(v["datacontenttype"], "application/json");
    assert_eq!(v["data"]["id"], 7);
    assert!(v["id"].as_str().is_some_and(|s| !s.is_empty()));
}

#[test]
fn two_events_get_different_ids() {
    assert_ne!(
        signal("x", "/y").unwrap().id(),
        signal("x", "/y").unwrap().id()
    );
}

#[test]
fn an_event_round_trips() {
    let e = event("t", "/s", &vec![1, 2, 3]).unwrap();
    let back: toolbox_cluster::CloudEvent =
        serde_json::from_str(&serde_json::to_string(&e).unwrap()).unwrap();
    assert_eq!(e.id(), back.id());
    assert_eq!(payload::<Vec<i32>>(&back).unwrap(), [1, 2, 3]);
}

#[test]
fn a_signal_carries_no_payload() {
    let e = signal("ping", "/s").unwrap();
    assert!(e.data().is_none());
    assert_eq!(e.ty(), "ping");
}

/// The envelope carries a timestamp, which is the SDK's job rather than ours -
/// there is no date arithmetic in this workspace.
#[test]
fn an_event_is_timestamped() {
    assert!(signal("x", "/y").unwrap().time().is_some());
}

#[tokio::test]
async fn a_published_event_reaches_a_subscriber() {
    let bus = InProcessEventBus::default();
    let topic = Topic::new("things");
    let mut stream = bus.subscribe(&topic, StartPosition::Now).await.unwrap();

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
    let bus = InProcessEventBus::default();
    let topic = Topic::new("things");
    let mut a = bus.subscribe(&topic, StartPosition::Now).await.unwrap();
    let mut b = bus.subscribe(&topic, StartPosition::Now).await.unwrap();

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
    let bus = InProcessEventBus::default();
    let mut stream = bus
        .subscribe(&Topic::new("a"), StartPosition::Now)
        .await
        .unwrap();
    bus.publish(&Topic::new("b"), signal("x", "/y").unwrap())
        .await
        .unwrap();

    let got = tokio::time::timeout(Duration::from_millis(100), stream.next()).await;
    assert!(got.is_err(), "nothing arrived, which is what should happen");
}

#[tokio::test]
async fn publishing_with_nobody_listening_is_not_an_error() {
    let bus = InProcessEventBus::default();
    assert!(
        bus.publish(&Topic::new("a"), signal("x", "/y").unwrap())
            .await
            .is_ok()
    );
}

/// A capability claim is a promise, so the adapter must reject what it says it
/// cannot do - at subscribe time, where it can be fixed.
#[tokio::test]
async fn an_adapter_without_replay_rejects_a_cursor_subscribe() {
    let bus = InProcessEventBus::default();
    assert!(bus.capabilities().replay.is_none());

    let result = bus
        .subscribe(&Topic::new("a"), StartPosition::Cursor("42".to_owned()))
        .await;
    match result {
        Err(EventBusError::Unsupported {
            needed: MissingCapability::Replay,
            adapter: "in-process",
        }) => {}
        Err(other) => panic!("wrong error: {other:?}"),
        Ok(_) => panic!("a cursor subscribe should be refused, not silently ignored"),
    }
}

#[tokio::test]
async fn an_adapter_without_replay_rejects_an_earliest_subscribe() {
    let bus = InProcessEventBus::default();
    let result = bus
        .subscribe(&Topic::new("a"), StartPosition::Earliest)
        .await;
    match result {
        Err(EventBusError::Unsupported {
            needed: MissingCapability::Replay,
            adapter: "in-process",
        }) => {}
        Err(other) => panic!("wrong error: {other:?}"),
        Ok(_) => panic!("`Earliest` has no history to give here, so it must be refused"),
    }
}

#[tokio::test]
async fn a_zero_buffer_is_clamped_rather_than_panicking() {
    let bus = InProcessEventBus::new(0);
    let topic = Topic::new("a");
    let mut stream = bus.subscribe(&topic, StartPosition::Now).await.unwrap();
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

#[tokio::test]
async fn the_in_process_bus_declares_what_it_actually_does() {
    let caps = InProcessEventBus::default().capabilities();
    assert_eq!(
        caps.delivery,
        Delivery::AtMostOnce,
        "a broadcast channel drops on lag"
    );
    assert!(!caps.durable, "nothing survives a restart");
    assert!(caps.replay.is_none());
}

#[test]
fn a_topic_is_its_name() {
    assert_eq!(Topic::from("things").as_str(), "things");
    assert_eq!(Topic::new("things").to_string(), "things");
}
