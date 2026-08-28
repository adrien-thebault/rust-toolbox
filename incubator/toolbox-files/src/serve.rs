//! Deciding how to answer a download.
//!
//! Caching, conditional requests, range requests and the headers that stop a
//! stored SVG running as script are four decisions with bad defaults, made once
//! here.
//!
//! **This returns a decision, not a response.** An `axum::response::Response`
//! would put axum's body type in the signature, and then a gRPC file service
//! could not use the same logic for its metadata.

use http::{HeaderMap, HeaderValue, StatusCode, header};

use crate::{error::FileError, meta::FileMeta};

/// How long an immutable file may be cached: one year, the maximum HTTP
/// defines as meaningful.
const IMMUTABLE_MAX_AGE: u32 = 31_536_000;

/// A byte range a client asked for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ByteRange {
    /// First byte, inclusive.
    pub start: u64,
    /// Last byte, inclusive.
    pub end: u64,
}

impl ByteRange {
    /// How many bytes this range covers.
    #[must_use]
    pub fn len(&self) -> u64 {
        self.end.saturating_sub(self.start) + 1
    }

    /// Whether the range covers nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.end < self.start
    }
}

/// What a caller conditioned their request on.
#[derive(Debug, Clone, Default)]
pub struct Conditionals {
    /// The `If-None-Match` header, verbatim.
    pub if_none_match: Option<String>,
}

/// What to answer, without saying how to build it.
#[derive(Debug, Clone)]
pub struct ServeDecision {
    /// The status to send.
    pub status: StatusCode,
    /// The headers to send.
    pub headers: HeaderMap,
    /// The byte range to read, or `None` for the whole file.
    ///
    /// `Some` with a `304` never happens: a not-modified response has no body.
    pub range: Option<ByteRange>,
}

impl ServeDecision {
    /// Whether this response carries a body.
    #[must_use]
    pub fn has_body(&self) -> bool {
        self.status != StatusCode::NOT_MODIFIED
    }
}

/// Decide how to answer a request for a file.
///
/// # Arguments
///
/// * `meta` - The stored file's identity: its hash, size and type, which is
///   everything the headers are built from.
/// * `range` - The byte range asked for, or `None` for the whole file.
/// * `conditionals` - What the caller conditioned the request on, which is what
///   turns a response into a 304.
///
/// # Errors
/// [`FileError::RangeNotSatisfiable`] when the requested range lies outside
/// the file.
pub fn serve(
    meta: &FileMeta,
    range: Option<ByteRange>,
    conditionals: &Conditionals,
) -> Result<ServeDecision, FileError> {
    let mut headers = base_headers(meta);

    // A content-addressed key cannot change content, so a matching ETag is
    // always still valid and the body never needs sending.
    if conditionals
        .if_none_match
        .as_deref()
        .is_some_and(|v| etag_matches(v, &meta.etag()))
    {
        return Ok(ServeDecision {
            status: StatusCode::NOT_MODIFIED,
            headers,
            range: None,
        });
    }

    let Some(range) = range else {
        insert(&mut headers, header::CONTENT_LENGTH, &meta.size.to_string());
        return Ok(ServeDecision {
            status: StatusCode::OK,
            headers,
            range: None,
        });
    };

    if range.is_empty() || range.start >= meta.size {
        return Err(FileError::RangeNotSatisfiable { size: meta.size });
    }
    // A client may ask past the end; the answer is the rest of the file, not
    // an error.
    let end = range.end.min(meta.size - 1);
    let clamped = ByteRange {
        start: range.start,
        end,
    };

    insert(
        &mut headers,
        header::CONTENT_RANGE,
        &format!("bytes {}-{}/{}", clamped.start, clamped.end, meta.size),
    );
    insert(
        &mut headers,
        header::CONTENT_LENGTH,
        &clamped.len().to_string(),
    );

    Ok(ServeDecision {
        status: StatusCode::PARTIAL_CONTENT,
        headers,
        range: Some(clamped),
    })
}

/// The headers every file response carries.
///
/// # Arguments
///
/// * `meta` - The file whose type, length, ETag and cache headers to emit.
fn base_headers(meta: &FileMeta) -> HeaderMap {
    let mut headers = HeaderMap::new();

    insert(&mut headers, header::CONTENT_TYPE, &meta.mime_type);
    insert(&mut headers, header::ETAG, &meta.etag());
    insert(&mut headers, header::ACCEPT_RANGES, "bytes");

    // The URL is the content's hash, so the bytes behind it can never change.
    insert(
        &mut headers,
        header::CACHE_CONTROL,
        &format!("public, max-age={IMMUTABLE_MAX_AGE}, immutable"),
    );

    // Two headers that stop a stored file from being a stored exploit. An SVG
    // or an HTML file served from your origin runs as your origin unless the
    // sandbox says otherwise, and a browser that sniffs past the declared type
    // will find script wherever it is hidden.
    insert(&mut headers, header::CONTENT_SECURITY_POLICY, "sandbox");
    insert(&mut headers, header::X_CONTENT_TYPE_OPTIONS, "nosniff");

    if let Some(name) = &meta.filename {
        insert(
            &mut headers,
            header::CONTENT_DISPOSITION,
            &content_disposition(name),
        );
    }
    headers
}

/// A `Content-Disposition` that cannot be used to inject header syntax.
///
/// The filename came from whoever uploaded it, so it is quoted, stripped of
/// anything that could break out, and truncated.
///
/// # Arguments
///
/// * `name` - The download name, as the uploader supplied it. It is quoted,
///   stripped and truncated, because it is attacker-controlled text going into
///   a header.
fn content_disposition(name: &str) -> String {
    let safe: String = name
        .chars()
        .filter(|c| !c.is_control() && *c != '"' && *c != '\\' && *c != ';')
        .take(128)
        .collect();
    if safe.is_empty() {
        "inline".to_owned()
    } else {
        format!("inline; filename=\"{safe}\"")
    }
}

/// Whether an `If-None-Match` value matches this ETag.
///
/// # Arguments
///
/// * `header_value` - The raw `If-None-Match` value, which may be a list, a
///   wildcard, or weak.
/// * `etag` - This file's ETag, derived from its content hash.
fn etag_matches(header_value: &str, etag: &str) -> bool {
    if header_value.trim() == "*" {
        return true;
    }
    header_value.split(',').map(str::trim).any(|candidate| {
        // A weak comparison is the right one for GET, and `W/"x"` and `"x"`
        // are the same entity for these purposes.
        candidate.trim_start_matches("W/") == etag
    })
}

/// Set a header from a string, skipping a value the header type cannot hold
/// rather than failing the response.
///
/// # Arguments
///
/// * `headers` - The map to write into.
/// * `name` - The header to set.
/// * `value` - Its value. An invalid one is dropped, because a missing header
///   beats a panicking download.
fn insert(headers: &mut HeaderMap, name: header::HeaderName, value: &str) {
    if let Ok(value) = HeaderValue::from_str(value) {
        headers.insert(name, value);
    }
}

/// Parse an HTTP `Range` header, accepting only the single-range forms.
///
/// Multi-range responses need `multipart/byteranges`, which is a lot of
/// machinery for something no common client sends. Returning `None` for those
/// means the whole file is served, which is always correct.
///
/// # Arguments
///
/// * `header_value` - The raw `Range` header. Multi-range forms give `None`,
///   which the caller answers with the whole file.
/// * `size` - The file's length, needed to resolve a suffix range and to reject
///   one that falls outside.
#[must_use]
pub fn parse_range(header_value: &str, size: u64) -> Option<ByteRange> {
    let spec = header_value.trim().strip_prefix("bytes=")?;
    if spec.contains(',') {
        return None;
    }
    let (start, end) = spec.split_once('-')?;

    match (start.trim(), end.trim()) {
        // `bytes=-500`: the last 500 bytes.
        ("", suffix) => {
            let n: u64 = suffix.parse().ok()?;
            let n = n.min(size);
            Some(ByteRange {
                start: size.checked_sub(n)?,
                end: size.checked_sub(1)?,
            })
        }
        // `bytes=500-`: from 500 to the end.
        (start, "") => Some(ByteRange {
            start: start.parse().ok()?,
            end: size.checked_sub(1)?,
        }),
        // `bytes=0-499`.
        (start, end) => Some(ByteRange {
            start: start.parse().ok()?,
            end: end.parse().ok()?,
        }),
    }
}
