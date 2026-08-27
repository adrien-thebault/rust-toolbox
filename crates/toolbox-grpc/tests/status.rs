use std::collections::BTreeMap;

use tonic::Code;
use toolbox_core::{ErrorKind, ServiceError};
use toolbox_grpc::{code_for, from_status, kind_for, to_status};

#[derive(Debug, thiserror::Error)]
#[error("event {id} not found")]
struct NotFound {
    id: i64,
}

impl ServiceError for NotFound {
    fn code(&self) -> &'static str {
        "EVENT_NOT_FOUND"
    }
    fn domain(&self) -> &'static str {
        "events"
    }
    fn kind(&self) -> ErrorKind {
        ErrorKind::NotFound
    }
    fn metadata(&self) -> BTreeMap<String, String> {
        BTreeMap::from([("id".to_owned(), self.id.to_string())])
    }
}

#[derive(Debug, thiserror::Error)]
#[error("connection refused to 10.0.0.4:5432 (password=hunter2)")]
struct Exploded;

impl ServiceError for Exploded {
    fn code(&self) -> &'static str {
        "DB_UNAVAILABLE"
    }
    fn domain(&self) -> &'static str {
        "events"
    }
    fn kind(&self) -> ErrorKind {
        ErrorKind::Internal
    }
}

#[test]
fn every_kind_maps_to_the_documented_code() {
    assert_eq!(code_for(ErrorKind::NotFound), Code::NotFound);
    assert_eq!(code_for(ErrorKind::InvalidArgument), Code::InvalidArgument);
    assert_eq!(code_for(ErrorKind::Unauthenticated), Code::Unauthenticated);
    assert_eq!(
        code_for(ErrorKind::PermissionDenied),
        Code::PermissionDenied
    );
    assert_eq!(code_for(ErrorKind::Conflict), Code::AlreadyExists);
    assert_eq!(
        code_for(ErrorKind::ResourceExhausted),
        Code::ResourceExhausted
    );
    assert_eq!(
        code_for(ErrorKind::FailedPrecondition),
        Code::FailedPrecondition
    );
    assert_eq!(code_for(ErrorKind::Timeout), Code::DeadlineExceeded);
    assert_eq!(code_for(ErrorKind::Unavailable), Code::Unavailable);
    assert_eq!(code_for(ErrorKind::Unimplemented), Code::Unimplemented);
    assert_eq!(code_for(ErrorKind::Internal), Code::Internal);
}

#[test]
fn the_reverse_mapping_round_trips_every_kind() {
    for kind in [
        ErrorKind::NotFound,
        ErrorKind::InvalidArgument,
        ErrorKind::Unauthenticated,
        ErrorKind::PermissionDenied,
        ErrorKind::Conflict,
        ErrorKind::ResourceExhausted,
        ErrorKind::FailedPrecondition,
        ErrorKind::Timeout,
        ErrorKind::Unavailable,
        ErrorKind::Unimplemented,
        ErrorKind::Internal,
    ] {
        assert_eq!(
            kind_for(code_for(kind)),
            kind,
            "{kind:?} did not survive the round trip"
        );
    }
}

/// The whole point of using `google.rpc.ErrorInfo`: a gateway can rebuild the
/// exact problem document the originating service would have produced.
#[test]
fn an_error_info_survives_the_status() {
    let status = to_status(NotFound { id: 7 });
    assert_eq!(status.code(), Code::NotFound);

    let info = from_status(&status).expect("the status carries an ErrorInfo");
    assert_eq!(info.code, "EVENT_NOT_FOUND");
    assert_eq!(info.domain, "events");
    assert_eq!(info.metadata["id"], "7");
}

#[test]
fn a_non_internal_message_is_the_errors_own_display() {
    assert_eq!(to_status(NotFound { id: 7 }).message(), "event 7 not found");
}

/// A gRPC message crosses to whoever called, so an internal failure's text -
/// which is database and dependency detail - must not be it.
#[test]
fn an_internal_message_is_replaced_rather_than_forwarded() {
    let status = to_status(Exploded);
    assert_eq!(status.code(), Code::Internal);
    assert_eq!(status.message(), "internal error");
    assert!(!status.message().contains("hunter2"));
    assert!(!status.message().contains("10.0.0.4"));

    // The stable code still travels, so a gateway can act on it.
    assert_eq!(from_status(&status).unwrap().code, "DB_UNAVAILABLE");
}

/// A status from a service that does not use this convention must read as
/// opaque rather than being guessed at.
#[test]
fn a_plain_status_carries_no_error_info() {
    assert!(from_status(&tonic::Status::not_found("gone")).is_none());
}

#[test]
fn an_unmapped_code_reads_as_internal() {
    assert_eq!(kind_for(Code::DataLoss), ErrorKind::Internal);
    assert_eq!(kind_for(Code::Unknown), ErrorKind::Internal);
    assert_eq!(kind_for(Code::Aborted), ErrorKind::Conflict);
    assert_eq!(kind_for(Code::OutOfRange), ErrorKind::InvalidArgument);
}
