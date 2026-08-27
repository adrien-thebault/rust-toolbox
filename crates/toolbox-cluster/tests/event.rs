use cloudevents::AttributesReader as _;
use toolbox_cluster::{event, payload, signal};

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
        "the field is `type`, not `type_`"
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
