use std::time::Duration;

use tonic::{Code, Status};
use toolbox_grpc::{
    BackendConfig, Discovery, MessageLimits, RetryPolicy, is_retryable, with_retry,
};

#[test]
fn a_config_defaults_to_no_retries_and_bounded_messages() {
    let cfg = BackendConfig::new("http://127.0.0.1:50051").unwrap();
    assert!(matches!(cfg.retry, RetryPolicy::None));
    assert_eq!(cfg.limits.max_decoding, 4 * 1024 * 1024);
    // tonic's own default is unlimited on the encoding side, which is the
    // asymmetry that produces "it works from the gateway but not the backend".
    assert_eq!(cfg.limits.max_encoding, 4 * 1024 * 1024);
}

#[test]
fn a_malformed_address_is_refused_at_construction() {
    assert!(BackendConfig::new("not a uri").is_err());
}

#[test]
fn discovery_can_be_static_dns_or_a_proxy() {
    let cfg = BackendConfig::new("http://a:1")
        .unwrap()
        .discovery(Discovery::Dns {
            name: "backend".to_owned(),
            port: 50051,
            refresh: Duration::from_secs(30),
        })
        .limits(MessageLimits {
            max_decoding: 1,
            max_encoding: 2,
        });

    assert!(matches!(cfg.discovery, Discovery::Dns { .. }));
    assert_eq!(cfg.limits.max_decoding, 1);
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
        backoff: toolbox_grpc::Backoff {
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
        backoff: toolbox_grpc::Backoff::default(),
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
        backoff: toolbox_grpc::Backoff {
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
        backoff: toolbox_grpc::Backoff {
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
