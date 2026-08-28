//! What is known about a stored file.

use serde::{Deserialize, Serialize};

/// The prefix content-addressed files are stored under.
pub const FILE_PREFIX: &str = "files";

/// The prefix an in-flight upload is written under before it is named.
pub const STAGING_PREFIX: &str = "staging";

/// A stored file's identity.
///
/// This is what `toolbox-file-service` owns. *Your* table owns file
/// **semantics** - who it belongs to, what it is for, who may read it - keyed
/// on `key`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileMeta {
    /// The content-addressed storage key: `files/<blake3>`.
    ///
    /// Content-addressed, which buys three things at once: identical uploads
    /// deduplicate, the URL is immutable so it can be cached forever, and the
    /// ETag *is* the key so conditional requests are correct for free.
    pub key: String,
    /// The blake3 hash of the content, in hex.
    pub hash: String,
    /// The name the client sent. A **label**, never trusted for anything.
    pub filename: Option<String>,
    /// The media type, **sniffed from the content**.
    pub mime_type: String,
    /// The size in bytes.
    pub size: u64,
}

impl FileMeta {
    /// The ETag for this file.
    ///
    /// Strong, and equal to the content hash: the content cannot change under
    /// a content-addressed key, so there is nothing to be weak about.
    #[must_use]
    pub fn etag(&self) -> String {
        format!("\"{}\"", self.hash)
    }
}

/// What `ingest` produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ingested {
    /// The file's identity.
    pub meta: FileMeta,
    /// Whether an identical file was already stored, so nothing was written.
    ///
    /// Worth surfacing: a caller counting storage should not count a
    /// deduplicated upload twice.
    pub deduplicated: bool,
}

/// The storage key for a hash.
///
/// # Arguments
///
/// * `hash` - The content hash. It is the file's name, which is what makes an
///   immutable cache header safe.
#[must_use]
pub fn key_for(hash: &str) -> String {
    format!("{FILE_PREFIX}/{hash}")
}
