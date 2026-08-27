use toolbox_cluster::{
    Adapter, Deployment, InMemoryKeyValue, InProcessBus, NullBus, Scope, check_deployment,
};

struct Shared;
impl Adapter for Shared {
    fn name(&self) -> &'static str {
        "SharedThing"
    }
    fn scope(&self) -> Scope {
        Scope::Shared
    }
}

fn clustered() -> Deployment {
    Deployment::Clustered {
        instance_id: "replica-2".to_owned(),
    }
}

#[test]
fn a_single_replica_accepts_every_adapter() {
    let bus = InProcessBus::default();
    let adapters: Vec<&dyn Adapter> = vec![&bus, &Shared];
    assert!(check_deployment(&Deployment::Single, &adapters).is_ok());
}

/// The whole point: an in-process bus under three replicas means a subscriber
/// never sees two thirds of the events, so the process must not start.
#[test]
fn a_local_adapter_under_clustering_refuses_to_start() {
    let bus = InProcessBus::default();
    let adapters: Vec<&dyn Adapter> = vec![&bus];
    let err = check_deployment(&clustered(), &adapters).unwrap_err();
    assert_eq!(err.adapters, ["InProcessBus"]);
    assert_eq!(err.count, 1);
}

/// The error has to name the variable to change, not just the problem.
#[test]
fn the_error_names_the_remedy() {
    let bus = InProcessBus::default();
    let adapters: Vec<&dyn Adapter> = vec![&bus];
    let err = check_deployment(&clustered(), &adapters).unwrap_err();
    let text = err.to_string();
    assert!(text.contains("InProcessBus"), "{text}");
    assert!(
        text.contains("EVENT_BUS"),
        "the message says what to change: {text}"
    );
}

/// Degraded is not broken. Refusing to boot for a per-process cache would make
/// the guard something people switch off, which costs you the cases where it
/// is right.
#[test]
fn a_degraded_adapter_under_clustering_warns_but_starts() {
    let kv = InMemoryKeyValue::default();
    assert!(matches!(kv.scope(), Scope::LocalDegraded { .. }));
    let adapters: Vec<&dyn Adapter> = vec![&kv];
    assert!(check_deployment(&clustered(), &adapters).is_ok());
}

#[test]
fn shared_adapters_pass_under_clustering() {
    let bus = NullBus;
    let adapters: Vec<&dyn Adapter> = vec![&bus, &Shared];
    assert!(check_deployment(&clustered(), &adapters).is_ok());
}

#[test]
fn every_failing_adapter_is_reported_at_once() {
    let bus = InProcessBus::default();
    let locks = toolbox_cluster::InProcessLocks::new();
    let adapters: Vec<&dyn Adapter> = vec![&bus, &locks, &Shared];
    let err = check_deployment(&clustered(), &adapters).unwrap_err();
    assert_eq!(
        err.count, 2,
        "one restart per problem is not a debugging loop anyone wants"
    );
    assert!(err.adapters.contains(&"InProcessBus"));
    assert!(err.adapters.contains(&"InProcessLocks"));
}

#[test]
fn a_deployment_reports_its_instance_id() {
    assert_eq!(clustered().instance_id(), Some("replica-2"));
    assert_eq!(Deployment::Single.instance_id(), None);
    assert!(!Deployment::Single.is_clustered());
}
