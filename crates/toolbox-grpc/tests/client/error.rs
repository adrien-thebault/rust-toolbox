use toolbox_core::{ErrorKind, ServiceError};
use toolbox_grpc::ClientError;

#[test]
fn a_bad_uri_is_a_client_mistake() {
    let err = ClientError::Uri("nope".to_owned());
    assert_eq!(err.kind(), ErrorKind::InvalidArgument);
    assert_eq!(err.code(), "INVALID_BACKEND_URI");
    assert_eq!(err.domain(), "grpc");
}

#[test]
fn an_unreachable_backend_is_transient() {
    let err = ClientError::Transport("boom".to_owned());
    assert_eq!(err.kind(), ErrorKind::Unavailable);
}
