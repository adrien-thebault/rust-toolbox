use std::time::Duration;

use tonic::{Code, Status};
use toolbox_grpc::{Backoff, RetryPolicy, is_retryable, with_retry};

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

/// Asking again will not make a `NotFound` become found.
#[test]
fn only_transient_codes_are_retryable() {
    assert!(is_retryable(Code::Unavailable));
    assert!(is_retryable(Code::DeadlineExceeded));
    assert!(is_retryable(Code::ResourceExhausted));

    assert!(!is_retryable(Code::NotFound));
    assert!(!is_retryable(Code::InvalidArgument));
    assert!(!is_retryable(Code::AlreadyExists));
    assert!(!is_retryable(Code::PermissionDenied));
}

#[tokio::test]
async fn with_retry_does_nothing_under_the_default_policy() {
    let mut calls = 0;
    let result: Result<(), Status> = with_retry(&RetryPolicy::None, "GetTodo", || {
        calls += 1;
        async { Err(Status::unavailable("down")) }
    })
    .await;

    assert!(result.is_err());
    assert_eq!(
        calls, 1,
        "the default policy is off, and off means one attempt"
    );
}

#[tokio::test]
async fn a_listed_method_is_retried_until_it_succeeds() {
    let policy = RetryPolicy::Idempotent {
        max_attempts: 3,
        backoff: Backoff {
            min_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(2),
            factor: 1.0,
            jitter: false,
        },
        methods: &["GetTodo"],
    };

    let mut calls = 0;
    let result: Result<u8, Status> = with_retry(&policy, "GetTodo", || {
        calls += 1;
        let attempt = calls;
        async move {
            if attempt < 3 {
                Err(Status::unavailable("down"))
            } else {
                Ok(7)
            }
        }
    })
    .await;

    assert_eq!(result.unwrap(), 7);
    assert_eq!(calls, 3);
}

/// A policy that silently duplicated a create would be worse than no policy,
/// which is why the methods have to be listed.
#[tokio::test]
async fn an_unlisted_method_is_not_retried() {
    let policy = RetryPolicy::Idempotent {
        max_attempts: 5,
        backoff: Backoff::default(),
        methods: &["GetTodo"],
    };

    let mut calls = 0;
    let _: Result<(), Status> = with_retry(&policy, "CreateTodo", || {
        calls += 1;
        async { Err(Status::unavailable("down")) }
    })
    .await;

    assert_eq!(calls, 1, "CreateTodo is not idempotent and was not listed");
}

#[tokio::test]
async fn a_non_retryable_failure_stops_immediately() {
    let policy = RetryPolicy::Idempotent {
        max_attempts: 5,
        backoff: Backoff {
            min_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(1),
            factor: 1.0,
            jitter: false,
        },
        methods: &["GetTodo"],
    };

    let mut calls = 0;
    let _: Result<(), Status> = with_retry(&policy, "GetTodo", || {
        calls += 1;
        async { Err(Status::not_found("gone")) }
    })
    .await;

    assert_eq!(calls, 1, "a NotFound will not become found");
}

#[tokio::test]
async fn retries_are_bounded_by_max_attempts() {
    let policy = RetryPolicy::Idempotent {
        max_attempts: 3,
        backoff: Backoff {
            min_delay: Duration::from_millis(1),
            max_delay: Duration::from_millis(1),
            factor: 1.0,
            jitter: false,
        },
        methods: &["GetTodo"],
    };

    let mut calls = 0;
    let result: Result<(), Status> = with_retry(&policy, "GetTodo", || {
        calls += 1;
        async { Err(Status::unavailable("down")) }
    })
    .await;

    assert_eq!(calls, 3);
    assert_eq!(
        result.unwrap_err().code(),
        Code::Unavailable,
        "the last error is returned"
    );
}
