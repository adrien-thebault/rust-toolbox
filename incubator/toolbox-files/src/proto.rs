//! File transfer over gRPC: the wire contract, and the two message builders.
//!
//! It sits between the storage half of this crate, which knows nothing about
//! gRPC, and a gateway, which knows nothing about object stores.

mod generated {
    #![allow(missing_docs, clippy::pedantic, clippy::all)]
    tonic::include_proto!("toolbox.v1");
}

pub use generated::{
    Chunk, DeleteRequest, DeleteResponse, DownloadRequest, FileInfo, StatRequest, UploadInfo,
    UploadRequest,
    file_service_client::FileServiceClient,
    file_service_server::{FileService, FileServiceServer},
    upload_request,
};

/// The chunk size every producer here uses.
///
/// 64 KiB: large enough that per-message overhead is negligible, small enough
/// that a hundred concurrent uploads cost megabytes rather than gigabytes.
pub const CHUNK_SIZE: usize = 64 * 1024;

/// Build the first message of an upload, which must carry the info.
///
/// # Arguments
///
/// * `filename` - The download name, kept for display. It never decides the
///   media type.
/// * `declared_mime` - What the client says it is. The service sniffs the bytes
///   anyway, so this is a hint rather than a fact.
#[must_use]
pub fn upload_info(filename: &str, declared_mime: &str) -> UploadRequest {
    UploadRequest {
        payload: Some(upload_request::Payload::Info(UploadInfo {
            filename: filename.to_owned(),
            declared_mime: declared_mime.to_owned(),
        })),
    }
}

/// Build a chunk message.
///
/// # Arguments
///
/// * `data` - One chunk of payload. [`CHUNK_SIZE`] is the size every producer
///   here uses.
#[must_use]
pub fn upload_chunk(data: bytes::Bytes) -> UploadRequest {
    UploadRequest {
        payload: Some(upload_request::Payload::Chunk(data)),
    }
}
