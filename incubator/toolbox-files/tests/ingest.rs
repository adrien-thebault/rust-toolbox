use std::sync::Arc;

use futures_util::StreamExt as _;
use object_store::{ObjectStore, memory::InMemory};
use toolbox_files::{FileError, MimePolicy, UploadPolicy, ingest, sniff};

use crate::{PNG, chunked, one};

fn store() -> Arc<dyn ObjectStore> {
    Arc::new(InMemory::new())
}

#[tokio::test]
async fn a_file_is_stored_under_the_hash_of_its_content() {
    let store = store();
    let out = ingest(
        store.as_ref(),
        one(b"hello"),
        &UploadPolicy::default(),
        None,
    )
    .await
    .unwrap();

    assert!(out.meta.key.starts_with("files/"));
    assert_eq!(out.meta.key, format!("files/{}", out.meta.hash));
    assert_eq!(out.meta.size, 5);
    assert!(!out.deduplicated);
}

/// Content addressing means an identical upload writes nothing, which is worth
/// surfacing so a caller counting storage does not count it twice.
#[tokio::test]
async fn an_identical_upload_deduplicates() {
    let store = store();
    let first = ingest(store.as_ref(), one(b"same"), &UploadPolicy::default(), None)
        .await
        .unwrap();
    let second = ingest(store.as_ref(), one(b"same"), &UploadPolicy::default(), None)
        .await
        .unwrap();

    assert_eq!(first.meta.key, second.meta.key);
    assert!(!first.deduplicated);
    assert!(second.deduplicated, "the second upload wrote nothing");
}

#[tokio::test]
async fn different_content_gets_a_different_key() {
    let store = store();
    let a = ingest(store.as_ref(), one(b"a"), &UploadPolicy::default(), None)
        .await
        .unwrap();
    let b = ingest(store.as_ref(), one(b"b"), &UploadPolicy::default(), None)
        .await
        .unwrap();
    assert_ne!(a.meta.key, b.meta.key);
}

/// The cap has to bite while bytes flow. Checking after buffering is a cap
/// that has already cost you the memory it was meant to protect.
#[tokio::test]
async fn an_oversized_upload_is_refused_part_way_through() {
    let store = store();
    let policy = UploadPolicy::default().max_bytes(100);
    let big = vec![0u8; 10_000];

    let err = ingest(store.as_ref(), chunked(&big, 16), &policy, None)
        .await
        .unwrap_err();
    assert!(matches!(err, FileError::TooLarge { max: 100 }), "{err:?}");
}

#[tokio::test]
async fn a_failed_upload_leaves_nothing_behind() {
    let store = store();
    let policy = UploadPolicy::default().max_bytes(10);
    let _ = ingest(store.as_ref(), chunked(&vec![0u8; 1000], 16), &policy, None).await;

    // Nothing staged, nothing stored: an attacker cancelling uploads must not
    // be able to fill the bucket for free.
    let mut listed = store.list(None);
    assert!(
        listed.next().await.is_none(),
        "the staged object was cleaned up"
    );
}

/// The declared filename is a claim; the bytes are not.
#[tokio::test]
async fn the_type_is_sniffed_rather_than_taken_from_the_filename() {
    let store = store();
    let out = ingest(
        store.as_ref(),
        one(PNG),
        &UploadPolicy::default(),
        Some("totally-a-document.pdf".to_owned()),
    )
    .await
    .unwrap();

    assert_eq!(
        out.meta.mime_type, "image/png",
        "the content decides, not the name"
    );
    assert_eq!(out.meta.filename.as_deref(), Some("totally-a-document.pdf"));
}

#[tokio::test]
async fn a_disallowed_type_is_refused_and_says_what_would_work() {
    let store = store();
    let policy = UploadPolicy::default().allowed(MimePolicy::Allowlist(&["application/pdf"]));

    let err = ingest(store.as_ref(), one(PNG), &policy, None)
        .await
        .unwrap_err();
    match err {
        FileError::UnsupportedType { found, allowed } => {
            assert_eq!(found, "image/png");
            assert_eq!(allowed, "application/pdf");
        }
        other => panic!("expected UnsupportedType, got {other:?}"),
    }
}

#[tokio::test]
async fn a_short_file_is_still_type_checked() {
    let store = store();
    let policy = UploadPolicy::images(1024);
    // Well under the sniff buffer, so it takes the short-file path.
    let err = ingest(store.as_ref(), one(b"hi"), &policy, None)
        .await
        .unwrap_err();
    assert!(matches!(err, FileError::UnsupportedType { .. }), "{err:?}");
}

#[tokio::test]
async fn a_chunked_upload_hashes_the_same_as_a_whole_one() {
    let store = store();
    let data = vec![7u8; 5000];
    let whole = ingest(store.as_ref(), one(&data), &UploadPolicy::default(), None)
        .await
        .unwrap();
    let split = ingest(
        store.as_ref(),
        chunked(&data, 13),
        &UploadPolicy::default(),
        None,
    )
    .await
    .unwrap();
    assert_eq!(whole.meta.hash, split.meta.hash);
    assert_eq!(whole.meta.size, split.meta.size);
}

#[test]
fn unknown_content_sniffs_as_a_generic_binary() {
    assert_eq!(
        sniff(b"not any known format at all"),
        "application/octet-stream"
    );
    assert_eq!(sniff(PNG), "image/png");
}
