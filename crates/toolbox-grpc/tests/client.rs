mod error;
mod retry;

use toolbox_grpc::{Backoff, ClientConfig, MessageLimits, RetryPolicy};

#[test]
fn a_config_defaults_to_no_retries_and_bounded_messages() {
    let cfg = ClientConfig::new("http://127.0.0.1:50051").unwrap();
    assert!(matches!(cfg.retry, RetryPolicy::None));
    assert_eq!(cfg.limits.max_decoding, 4 * 1024 * 1024);
    // tonic's own default is unlimited on the encoding side, which is the
    // asymmetry that produces "it works from the gateway but not the backend".
    assert_eq!(cfg.limits.max_encoding, 4 * 1024 * 1024);
}

#[test]
fn a_malformed_address_is_refused_at_construction() {
    assert!(ClientConfig::new("not a uri").is_err());
}

#[test]
fn limits_a_secret_and_a_retry_policy_are_builder_set() {
    let cfg = ClientConfig::new("http://a:1")
        .unwrap()
        .limits(MessageLimits {
            max_decoding: 1,
            max_encoding: 2,
        })
        .service_secret("s3cr3t")
        .retry(RetryPolicy::Idempotent {
            max_attempts: 2,
            backoff: Backoff::default(),
            methods: &["GetTodo"],
        });

    assert_eq!(cfg.limits.max_decoding, 1);
    assert!(cfg.service_secret.is_some());
    assert!(cfg.retry.allows("GetTodo"));
}
