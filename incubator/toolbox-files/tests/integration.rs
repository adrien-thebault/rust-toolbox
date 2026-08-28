//! One harness per crate; the module tree mirrors `src/`.
#![allow(missing_docs, clippy::missing_panics_doc)]

mod ingest;
mod policy;
mod proto;
mod serve;

use bytes::Bytes;
use futures_util::stream;

/// A stream of one chunk.
pub fn one(data: &[u8]) -> impl futures_core::Stream<Item = Result<Bytes, std::io::Error>> + Send {
    stream::iter(vec![Ok(Bytes::copy_from_slice(data))])
}

/// A stream of many small chunks, which is what a real upload looks like.
pub fn chunked(
    data: &[u8],
    chunk: usize,
) -> impl futures_core::Stream<Item = Result<Bytes, std::io::Error>> + Send {
    let chunks: Vec<Result<Bytes, std::io::Error>> = data
        .chunks(chunk)
        .map(|c| Ok(Bytes::copy_from_slice(c)))
        .collect();
    stream::iter(chunks)
}

/// A minimal but genuine PNG, so `infer` recognises it.
pub const PNG: &[u8] = &[
    0x89, 0x50, 0x4E, 0x47, 0x0D, 0x0A, 0x1A, 0x0A, 0x00, 0x00, 0x00, 0x0D, 0x49, 0x48, 0x44, 0x52,
    0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x06, 0x00, 0x00, 0x00, 0x1F, 0x15, 0xC4,
    0x89,
];
mod service;
