//! Streaming files between HTTP and gRPC.
//!
//! These bridge axum's multipart, tonic's streaming and this crate's storage
//! half, none
//! of which know about each other, and they are fiddly to get right without
//! buffering.
//!
//! **This is the only code that needs both transports**, which is why it sits
//! behind the `web` feature: `toolbox-web` itself never depends on
//! `toolbox-grpc`.
//!
//! Nothing on this path holds the whole file: a 100 MB upload costs one 64 KiB
//! buffer. That is what makes the message-size apparatus unnecessary -
//! `MAX_FILE_SIZE`, `GRPC_MESSAGE_SIZE_LIMIT`, `UPLOAD_BODY_LIMIT` and
//! `max_encoding_message_size` at both ends only ever existed to let whole
//! files be single messages.

use axum::{body::Body, extract::multipart::Field, response::Response};
use bytes::Bytes;
use futures_core::Stream;
use futures_util::StreamExt as _;
use tonic::Streaming;

use crate::{
    proto::{UploadRequest, upload_chunk, upload_info},
    serve::ServeDecision,
};

/// Turn a multipart field into the request stream `FileService::Upload` wants.
///
/// The first message carries the filename and declared type; every message
/// after it is a chunk, forwarded as it arrives.
///
/// # Arguments
///
/// * `field` - The multipart field being uploaded. It is forwarded chunk by
///   chunk, so nothing is buffered in the gateway.
pub fn multipart_to_upload_stream(
    field: Field<'static>,
) -> impl Stream<Item = UploadRequest> + Send {
    let filename = field.file_name().unwrap_or_default().to_owned();
    let declared = field.content_type().unwrap_or_default().to_owned();

    let head = futures_util::stream::once(async move { upload_info(&filename, &declared) });
    // A chunk that fails mid-stream ends the stream. The server sees a
    // truncated upload and fails it, which is the correct outcome: half a file
    // must not be stored under a hash of half a file.
    let body = field.filter_map(|chunk| async move { chunk.ok().map(upload_chunk) });

    head.chain(body)
}

/// Turn a gRPC download stream into an HTTP response body.
///
/// The headers come from [`crate::serve::serve()`], so an HTTP download and a
/// gRPC one agree on caching, ETags and the security headers.
///
/// # Arguments
///
/// * `stream` - The backend's chunk stream, forwarded as it arrives.
/// * `decision` - What `crate::serve::serve()` decided, which is where the
///   status and headers come from.
///
/// # Errors
/// Never; a stream error becomes an error item in the body, because the
/// status line has already been sent by then.
#[must_use]
pub fn download_stream_to_body(
    stream: Streaming<crate::proto::Chunk>,
    decision: &ServeDecision,
) -> Response {
    let body = Body::from_stream(stream.map(|chunk| {
        chunk
            .map(|c| c.data)
            .map_err(|e| std::io::Error::other(e.message().to_owned()))
    }));

    let mut response = Response::new(body);
    *response.status_mut() = decision.status;
    *response.headers_mut() = decision.headers.clone();
    response
}

/// Build the response for a locally stored file.
///
/// Takes only the decision: [`crate::serve::serve()`] already folded the
/// metadata into its status and headers, so passing the `FileMeta` as well
/// would be a parameter the function does not read.
///
/// # Arguments
///
/// * `decision` - The status and headers already folded together by
///   `crate::serve::serve()`.
/// * `body` - The bytes to send. Ignored for a 304, which has no body.
#[must_use]
pub fn serve_response<S>(decision: &ServeDecision, body: S) -> Response
where
    S: Stream<Item = Result<Bytes, crate::FileError>> + Send + 'static,
{
    // A 304 carries no body, and sending one would be a protocol error rather
    // than merely wasteful.
    let body = if decision.has_body() {
        Body::from_stream(body.map(|c| c.map_err(|e| std::io::Error::other(e.to_string()))))
    } else {
        Body::empty()
    };

    let mut response = Response::new(body);
    *response.status_mut() = decision.status;
    *response.headers_mut() = decision.headers.clone();
    response
}
