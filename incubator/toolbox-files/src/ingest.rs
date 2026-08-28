//! Taking bytes in.
//!
//! The obvious implementation buffers the body, then checks the size, then
//! hashes, then writes - so a 2 GB upload costs 2 GB of memory before the cap
//! that would have rejected it is consulted. This makes one pass with a fixed
//! buffer and fails at the byte that crosses the cap.

use bytes::Bytes;
use futures_core::Stream;
use futures_util::StreamExt as _;
use object_store::{ObjectStore, ObjectStoreExt as _, PutPayload, path::Path};

use crate::{
    error::FileError,
    meta::{FileMeta, Ingested, STAGING_PREFIX, key_for},
    policy::UploadPolicy,
};

/// How many bytes are needed to sniff a media type.
///
/// `infer` reads magic bytes from the head of the file; 8 KiB covers every
/// signature it knows and is one buffer.
const SNIFF_BYTES: usize = 8192;

/// Read a stream into the store, enforcing the policy as bytes flow.
///
/// The stream is staged under a random key while being hashed, then promoted
/// to its content-addressed name. Writing straight to the final key is
/// impossible: the key *is* the hash, and the hash is not known until the last
/// byte.
///
/// # Arguments
///
/// * `store` - Where the bytes go. Any `object_store` backend: local disk in
///   development, S3 in production.
/// * `body` - The incoming bytes. Consumed as they arrive, so an over-large
///   upload is refused mid-stream rather than after it lands.
/// * `policy` - The size cap and media-type allowlist, enforced against the
///   sniffed type.
/// * `filename` - What the client called it, kept for the download name only.
///   It never decides the media type.
///
/// # Errors
/// [`FileError::TooLarge`] at the byte that crosses the cap,
/// [`FileError::UnsupportedType`] once enough bytes exist to sniff, or
/// [`FileError::Store`] when the object store fails.
pub async fn ingest<S, E>(
    store: &dyn ObjectStore,
    body: S,
    policy: &UploadPolicy,
    filename: Option<String>,
) -> Result<Ingested, FileError>
where
    S: Stream<Item = Result<Bytes, E>> + Send,
    E: std::fmt::Display,
{
    let staging = Path::from(format!("{STAGING_PREFIX}/{}", uuid::Uuid::now_v7()));
    let result = stream_to_staging(store, body, policy, &staging).await;

    let (hash, size, mime) = match result {
        Ok(v) => v,
        Err(e) => {
            // Leaving a partial object behind would accumulate silently, and
            // an attacker who cancels uploads should not be able to fill the
            // bucket for free.
            let _ = store.delete(&staging).await;
            return Err(e);
        }
    };

    let key = key_for(&hash);
    let final_path = Path::from(key.clone());

    // Identical content is already there under the same name, so there is
    // nothing to write. This is what content addressing buys.
    let deduplicated = store.head(&final_path).await.is_ok();
    if deduplicated {
        let _ = store.delete(&staging).await;
    } else {
        store.copy(&staging, &final_path).await?;
        let _ = store.delete(&staging).await;
    }

    Ok(Ingested {
        meta: FileMeta {
            key,
            hash,
            filename,
            mime_type: mime,
            size,
        },
        deduplicated,
    })
}

/// Stream into staging, returning the hash, the size and the sniffed type.
///
/// # Arguments
///
/// * `store` - Where the bytes go. Any `object_store` backend: local disk in
///   development, S3 in production.
/// * `body` - The incoming bytes.
/// * `policy` - The limits to enforce while they flow.
/// * `staging` - The random key to write under. Promotion to the
///   content-addressed name happens afterwards, so a partial upload is never
///   visible at the real key.
async fn stream_to_staging<S, E>(
    store: &dyn ObjectStore,
    body: S,
    policy: &UploadPolicy,
    staging: &Path,
) -> Result<(String, u64, String), FileError>
where
    S: Stream<Item = Result<Bytes, E>> + Send,
    E: std::fmt::Display,
{
    let mut upload = store.put_multipart(staging).await?;
    let mut hasher = blake3::Hasher::new();
    let mut head = Vec::with_capacity(SNIFF_BYTES);
    let mut size: u64 = 0;

    let mut body = std::pin::pin!(body);
    while let Some(chunk) = body.next().await {
        let chunk = chunk.map_err(|e| FileError::Stream(e.to_string()))?;

        size += chunk.len() as u64;
        if size > policy.max_bytes {
            // Before writing the chunk that crossed it, and before reading
            // another.
            let _ = upload.abort().await;
            return Err(FileError::TooLarge {
                max: policy.max_bytes,
            });
        }

        if head.len() < SNIFF_BYTES {
            let want = SNIFF_BYTES - head.len();
            head.extend_from_slice(&chunk[..want.min(chunk.len())]);
            // As soon as there is enough to sniff, check the policy: rejecting
            // after streaming a gigabyte of the wrong type is a cap that did
            // not do its job.
            if head.len() >= SNIFF_BYTES
                && let Err(e) = check_type(&head, policy)
            {
                let _ = upload.abort().await;
                return Err(e);
            }
        }

        hasher.update(&chunk);
        upload.put_part(PutPayload::from_bytes(chunk)).await?;
    }

    // A short file never reached SNIFF_BYTES, so it is checked here.
    if head.len() < SNIFF_BYTES
        && let Err(e) = check_type(&head, policy)
    {
        let _ = upload.abort().await;
        return Err(e);
    }

    upload.complete().await?;
    Ok((hasher.finalize().to_hex().to_string(), size, sniff(&head)))
}

/// The media type of some content, from its magic bytes.
///
/// **Sniffed, never taken from the filename.** A `.png` extension is a claim
/// by whoever uploaded it; the bytes are not.
///
/// # Arguments
///
/// * `head` - The first bytes of the content. Magic numbers live at the front,
///   so the whole file is not needed.
#[must_use]
pub fn sniff(head: &[u8]) -> String {
    infer::get(head).map_or_else(
        || "application/octet-stream".to_owned(),
        |kind| kind.mime_type().to_owned(),
    )
}

/// Reject content whose sniffed type the policy does not allow.
///
/// # Arguments
///
/// * `head` - The first bytes, from which the real type is read.
/// * `policy` - The allowlist to check against.
fn check_type(head: &[u8], policy: &UploadPolicy) -> Result<(), FileError> {
    let mime = sniff(head);
    if policy.allowed.permits(&mime) {
        return Ok(());
    }
    Err(FileError::UnsupportedType {
        found: mime,
        allowed: policy.allowed.allowed().join(", "),
    })
}
