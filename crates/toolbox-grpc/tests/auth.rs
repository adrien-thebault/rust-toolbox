use toolbox_grpc::{SERVICE_AUTH_HEADER, ServiceAuth};

#[test]
fn the_header_is_the_one_the_layer_reads() {
    assert_eq!(SERVICE_AUTH_HEADER.as_str(), "x-service-auth");
}

#[test]
fn a_shared_secret_becomes_a_header_value() {
    let auth = ServiceAuth::shared_secret("s3cret");
    assert_eq!(auth.header_value().unwrap(), "s3cret");
}

/// A Debug of a config struct ends up in logs, so the secret must not be in it.
#[test]
fn debug_never_prints_the_secret() {
    let rendered = format!("{:?}", ServiceAuth::shared_secret("s3cret"));
    assert!(!rendered.contains("s3cret"), "{rendered}");
    assert!(rendered.contains("redacted"), "{rendered}");
}
