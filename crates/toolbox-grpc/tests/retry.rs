use toolbox_grpc::{Backoff, RetryPolicy};

/// A policy that silently duplicates a create is worse than no retry at all,
/// so the default is off and the methods must be listed.
#[test]
fn the_default_policy_retries_nothing() {
    let policy = RetryPolicy::default();
    assert!(matches!(policy, RetryPolicy::None));
    assert!(!policy.allows("GetEvent"));
    assert_eq!(policy.attempts("GetEvent"), 1);
}

#[test]
fn only_the_listed_methods_are_retried() {
    let policy = RetryPolicy::Idempotent {
        max_attempts: 3,
        backoff: Backoff::default(),
        methods: &["GetEvent", "ListEvents"],
    };

    assert!(policy.allows("GetEvent"));
    assert_eq!(policy.attempts("GetEvent"), 3);

    assert!(!policy.allows("CreateEvent"), "a create is not idempotent");
    assert_eq!(policy.attempts("CreateEvent"), 1);
}

#[test]
fn the_default_backoff_jitters_so_a_fleet_does_not_retry_in_lockstep() {
    let backoff = Backoff::default();
    assert!(backoff.jitter);
    assert!(backoff.min_delay < backoff.max_delay);
    assert!(backoff.factor > 1.0);
}
