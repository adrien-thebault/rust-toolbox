//! propagates the ambient [`CURRENT_REQUEST_ID`] set by
//! [`request_id_context_layer`](crate::tower_tools::layers::request_id_context_layer)
//! onto outgoing gRPC calls, so a callee's own
//! [`request_id_layer`](crate::tower_tools::layers::request_id_layer) keeps
//! it instead of minting a fresh one for that hop.

use crate::tower_tools::layers::{CURRENT_REQUEST_ID, X_REQUEST_ID_STR};
use tonic::service::interceptor::InterceptedService;
use tonic::transport::Channel;
use tonic::{Request, Status, metadata::MetadataValue};

/// stamps the ambient [`CURRENT_REQUEST_ID`] (if any is set for the
/// request currently being handled) onto an outgoing gRPC call's metadata,
/// under [`X_REQUEST_ID_STR`]. Pass to a generated client's
/// `with_interceptor` - see [`RequestIdChannel`]. A no-op if nothing is
/// currently scoped in [`CURRENT_REQUEST_ID`] (e.g. called from outside a
/// `request_id_context_layer`-wrapped request).
pub fn request_id_interceptor(mut req: Request<()>) -> Result<Request<()>, Status> {
    if let Ok(Some(value)) =
        CURRENT_REQUEST_ID.try_with(|id| MetadataValue::try_from(id.header_value().as_bytes()).ok())
    {
        req.metadata_mut().insert(X_REQUEST_ID_STR, value);
    }
    Ok(req)
}

/// the type of a [`Channel`] wrapped with [`request_id_interceptor`] via a
/// generated client's `with_interceptor`
pub type RequestIdChannel =
    InterceptedService<Channel, fn(Request<()>) -> Result<Request<()>, Status>>;
