//! Adapting a chunked upload stream into [`ingest`](fn@crate::ingest).
//!
//! It bridges a transport that delivers framed messages and an ingest path that
//! wants raw bytes, without either learning about the other, and without
//! anything on the path holding the whole file.

use bytes::Bytes;
use futures_core::Stream;
use futures_util::StreamExt as _;
use object_store::{ObjectStore, ObjectStoreExt as _};

use crate::{error::FileError, meta::Ingested, policy::UploadPolicy};

/// Ingest a stream of already-unframed chunks.
///
/// The framing - a first message carrying the filename, then chunk messages -
/// is the transport's business, so the caller unwraps it and hands the bytes
/// here. A 100 MB upload costs one 64 KiB buffer.
///
/// # Arguments
///
/// * `store` - Where the bytes go. Any `object_store` backend: local disk in
///   development, S3 in production.
/// * `chunks` - The payload bytes, already unwrapped from whatever framing the
///   transport used.
/// * `policy` - The limits to enforce as they flow.
/// * `filename` - The download name from the first message, kept for display
///   only.
///
/// # Errors
/// As [`crate::ingest()`].
pub async fn ingest_stream<S, E>(
    store: &dyn ObjectStore,
    chunks: S,
    policy: &UploadPolicy,
    filename: Option<String>,
) -> Result<Ingested, FileError>
where
    S: Stream<Item = Result<Bytes, E>> + Send,
    E: std::fmt::Display,
{
    crate::ingest::ingest(store, chunks, policy, filename).await
}

/// Read a stored file back as a stream of chunks.
///
/// Takes an owned store handle and key so the returned stream is `'static`:
/// a gRPC server stream outlives the handler that built it, so a borrowed one
/// cannot be returned.
///
/// # Arguments
///
/// * `store` - An owned handle, so the returned stream is `'static` and can
///   outlive the handler that built it.
/// * `key` - The content-addressed key to read.
/// * `range` - The byte range to read, or `None` for the whole file.
///
/// # Errors
/// [`FileError::NotFound`] when the key is unknown, or [`FileError::Store`]
/// when the object store fails.
pub async fn read_stream(
    store: std::sync::Arc<dyn ObjectStore>,
    key: &str,
    range: Option<crate::serve::ByteRange>,
) -> Result<impl Stream<Item = Result<Bytes, FileError>> + Send + 'static, FileError> {
    let path = object_store::path::Path::from(key);
    let result = match range {
        Some(r) => {
            let end = r
                .end
                .checked_add(1)
                .ok_or(FileError::RangeNotSatisfiable { size: r.end })?;
            store
                .get_range(&path, r.start..end)
                .await
                .map(|bytes| futures_util::stream::once(async move { Ok(bytes) }).boxed())?
        }
        None => store
            .get(&path)
            .await?
            .into_stream()
            .map(|chunk| chunk.map_err(FileError::from))
            .boxed(),
    };
    Ok(result)
}
