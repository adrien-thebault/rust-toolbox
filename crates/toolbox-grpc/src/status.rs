//! Converting between domain errors and `tonic::Status`.
//!
//! It bridges `toolbox-core`'s transport-neutral error vocabulary and
//! `google.rpc.ErrorInfo` as carried by `tonic-types`, which do not know about
//! each other.

use tonic::{Code, Status};
use tonic_types::{ErrorDetails, StatusExt as _};
use toolbox_core::{ErrorInfo, ErrorKind, ServiceError};

/// What a gRPC handler returns.
///
/// **A plain type alias, and nothing more.** There is deliberately no blanket
/// `impl<E: ServiceError> From<E> for tonic::Status`: `Status` is foreign and
/// `E` is a type parameter, so that impl violates the orphan rule here exactly
/// as it would in any consumer crate. Every attempt ends in `E0117`.
///
/// Each consumer instead keeps the one line
///
/// ```ignore
/// impl From<EventError> for Status {
///     fn from(e: EventError) -> Self { toolbox_grpc::to_status(e) }
/// }
/// ```
///
/// which is legal because `EventError` is theirs. The alternative that would
/// work - a toolbox-local error type in the return position - is not available
/// either, because tonic's generated trait fixes the signature.
pub type GrpcResult<T> = Result<tonic::Response<T>, Status>;

/// The one mapping from a transport-neutral kind to a gRPC code.
///
/// # Arguments
///
/// * `kind` - The transport-neutral failure kind to translate.
#[must_use]
pub fn code_for(kind: ErrorKind) -> Code {
    match kind {
        ErrorKind::NotFound => Code::NotFound,
        ErrorKind::InvalidArgument => Code::InvalidArgument,
        ErrorKind::Unauthenticated => Code::Unauthenticated,
        ErrorKind::PermissionDenied => Code::PermissionDenied,
        ErrorKind::Conflict => Code::AlreadyExists,
        ErrorKind::ResourceExhausted => Code::ResourceExhausted,
        ErrorKind::FailedPrecondition => Code::FailedPrecondition,
        ErrorKind::Timeout => Code::DeadlineExceeded,
        ErrorKind::Unavailable => Code::Unavailable,
        ErrorKind::Unimplemented => Code::Unimplemented,
        // ErrorKind is #[non_exhaustive]: a kind added upstream lands on the
        // same arm as Internal rather than breaking the build.
        ErrorKind::Internal | _ => Code::Internal,
    }
}

/// The reverse mapping, for a gateway decoding a backend's failure.
///
/// # Arguments
///
/// * `code` - The status a backend returned.
#[must_use]
pub fn kind_for(code: Code) -> ErrorKind {
    match code {
        Code::NotFound => ErrorKind::NotFound,
        Code::InvalidArgument | Code::OutOfRange => ErrorKind::InvalidArgument,
        Code::Unauthenticated => ErrorKind::Unauthenticated,
        Code::PermissionDenied => ErrorKind::PermissionDenied,
        Code::AlreadyExists | Code::Aborted => ErrorKind::Conflict,
        Code::ResourceExhausted => ErrorKind::ResourceExhausted,
        Code::FailedPrecondition => ErrorKind::FailedPrecondition,
        Code::DeadlineExceeded => ErrorKind::Timeout,
        Code::Unavailable => ErrorKind::Unavailable,
        Code::Unimplemented => ErrorKind::Unimplemented,
        _ => ErrorKind::Internal,
    }
}

/// Turn a domain error into a `Status` carrying its `ErrorInfo`.
///
/// The code, domain and metadata travel as `google.rpc.ErrorInfo` details, so
/// a gateway can rebuild the exact same problem document the originating
/// service would have produced.
///
/// The message is the error's `Display` **except on `Internal`**, where it is
/// replaced: an internal failure's text is database or dependency detail, and a
/// gRPC message crosses to whoever called.
///
/// # Arguments
///
/// * `err` - The error to convert. Taken by value because the caller is almost
///   always an `impl From<XError> for Status`, which owns it and has no use for
///   it afterwards.
#[allow(clippy::needless_pass_by_value)]
pub fn to_status<E: ServiceError>(err: E) -> Status {
    let kind = err.kind();
    let info = err.info();
    let details = ErrorDetails::with_error_info(
        info.code.clone(),
        info.domain.clone(),
        // ErrorInfo keeps a BTreeMap so its JSON is deterministic; tonic-types
        // wants a HashMap, and the ordering does not survive the wire anyway.
        info.metadata
            .into_iter()
            .collect::<std::collections::HashMap<_, _>>(),
    );

    let message = if kind == ErrorKind::Internal {
        "internal error".to_owned()
    } else {
        err.to_string()
    };

    Status::with_error_details(code_for(kind), message, details)
}

/// Read the `ErrorInfo` a `Status` carries, if it has one.
///
/// Returns `None` for a status from a service that does not use this
/// convention, which a caller should treat as an opaque failure rather than
/// guessing at a code.
///
/// # Arguments
///
/// * `status` - The status to read. One from a service that does not follow
///   this convention gives `None`, which a caller should treat as an opaque
///   failure rather than invent a code for.
#[must_use]
pub fn from_status(status: &Status) -> Option<ErrorInfo> {
    let details = status.get_error_details();
    details.error_info().map(|info| ErrorInfo {
        code: info.reason.clone(),
        domain: info.domain.clone(),
        metadata: info.metadata.clone().into_iter().collect(),
    })
}
