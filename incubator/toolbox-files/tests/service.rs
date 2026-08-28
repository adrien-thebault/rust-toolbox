use std::sync::Arc;

use bytes::Bytes;
use diesel::sqlite::SqliteConnection;
use futures_util::StreamExt as _;
use object_store::{ObjectStore, memory::InMemory};
use toolbox_files::{
    proto::{DeleteRequest, DownloadRequest, FileServiceClient, StatRequest, UploadRequest},
    service::{MIGRATIONS, builder, records::FileRecords},
    upload_chunk, upload_info,
};
use toolbox_grpc::{BackendConfig, backend};
use toolbox_test::{TestCluster, temp_db};

// The consumer's half: the component ships the schema and the service, the
// consumer generates the queries for its own backend.
toolbox_files::diesel_file_records!(FileStore, diesel::sqlite::Sqlite, SqliteConnection);

async fn records() -> (Arc<FileStore>, toolbox_test::db::TempDb) {
    let (db, guard) = temp_db::<SqliteConnection>();
    db.migrate(MIGRATIONS)
        .await
        .expect("the component's migrations");
    (Arc::new(FileStore::new(db)), guard)
}

/// A mounted service on a real socket, plus a client for it.
async fn mounted() -> (
    FileServiceClient<toolbox_grpc::BackendService>,
    TestCluster,
    toolbox_test::db::TempDb,
) {
    let (records, guard) = records().await;
    let store: Arc<dyn ObjectStore> = Arc::new(InMemory::new());

    let cluster = TestCluster::new()
        .service("files", move |routes| {
            routes.add_service(builder(records, store).build());
        })
        .await
        .expect("the file service came up");

    let channel = backend(
        "files",
        &BackendConfig::new(&cluster.backends().uri("files")).expect("a valid uri"),
    )
    .await
    .expect("a channel");

    (FileServiceClient::new(channel.channel()), cluster, guard)
}

fn upload_messages(filename: &str, data: &[u8]) -> Vec<UploadRequest> {
    let mut messages = vec![upload_info(filename, "application/octet-stream")];
    for chunk in data.chunks(8) {
        messages.push(upload_chunk(Bytes::copy_from_slice(chunk)));
    }
    messages
}

#[tokio::test]
async fn the_migrations_are_prefixed_so_they_cannot_collide() {
    use diesel::connection::SimpleConnection as _;

    let (db, _guard) = temp_db::<SqliteConnection>();
    db.migrate(MIGRATIONS).await.unwrap();
    db.blocking_conn()
        .unwrap()
        .batch_execute("SELECT key, hash, mime_type, size FROM toolbox_files")
        .expect("the component's table exists under its prefixed name");
}

#[tokio::test]
async fn a_recorded_file_can_be_read_back() {
    let (records, _guard) = records().await;
    let meta = toolbox_files::FileMeta {
        key: "files/abc".to_owned(),
        hash: "abc".to_owned(),
        filename: Some("x.bin".to_owned()),
        mime_type: "application/octet-stream".to_owned(),
        size: 3,
    };

    records.record(&meta).await.unwrap();
    assert_eq!(
        records.get("files/abc").await.unwrap().as_ref(),
        Some(&meta)
    );
    assert!(records.get("files/missing").await.unwrap().is_none());
}

#[tokio::test]
async fn recording_the_same_key_twice_is_not_a_duplicate() {
    let (records, _guard) = records().await;
    let meta = toolbox_files::FileMeta {
        key: "files/abc".to_owned(),
        hash: "abc".to_owned(),
        filename: None,
        mime_type: "text/plain".to_owned(),
        size: 1,
    };
    records.record(&meta).await.unwrap();
    records.record(&meta).await.unwrap();
    assert!(records.get("files/abc").await.unwrap().is_some());
}

#[tokio::test]
async fn deleting_marks_the_record_and_is_idempotent() {
    let (records, _guard) = records().await;
    let meta = toolbox_files::FileMeta {
        key: "files/abc".to_owned(),
        hash: "abc".to_owned(),
        filename: None,
        mime_type: "text/plain".to_owned(),
        size: 1,
    };
    records.record(&meta).await.unwrap();

    assert!(records.delete("files/abc").await.unwrap());
    assert!(
        !records.delete("files/abc").await.unwrap(),
        "a second delete changes nothing"
    );
    assert!(records.get("files/abc").await.unwrap().is_none());
}

/// Re-uploading identical content after a delete restores it, because the
/// bytes never went away - which is what content addressing means.
#[tokio::test]
async fn re_recording_a_deleted_key_undeletes_it() {
    let (records, _guard) = records().await;
    let meta = toolbox_files::FileMeta {
        key: "files/abc".to_owned(),
        hash: "abc".to_owned(),
        filename: None,
        mime_type: "text/plain".to_owned(),
        size: 1,
    };
    records.record(&meta).await.unwrap();
    records.delete("files/abc").await.unwrap();
    records.record(&meta).await.unwrap();
    assert!(records.get("files/abc").await.unwrap().is_some());
}

#[tokio::test]
async fn an_upload_round_trips_over_a_real_wire() {
    let (mut client, _cluster, _guard) = mounted().await;
    let data = b"the quick brown fox jumps over the lazy dog";

    let meta = client
        .upload(tokio_stream::iter(upload_messages("fox.txt", data)))
        .await
        .unwrap()
        .into_inner();

    assert_eq!(meta.size, data.len() as u64);
    assert_eq!(meta.filename, "fox.txt");
    assert!(meta.key.starts_with("files/"), "{}", meta.key);
    assert!(!meta.deduplicated);

    let stat = client
        .stat(StatRequest {
            key: meta.key.clone(),
        })
        .await
        .unwrap()
        .into_inner();
    assert_eq!(stat.hash, meta.hash);
    assert_eq!(stat.size, meta.size);

    let mut download = client
        .download(DownloadRequest {
            key: meta.key.clone(),
            range_start: None,
            range_end: None,
        })
        .await
        .unwrap()
        .into_inner();

    let mut out = Vec::new();
    while let Some(chunk) = download.next().await {
        out.extend_from_slice(&chunk.unwrap().data);
    }
    assert_eq!(
        out, data,
        "the bytes came back unchanged through both directions"
    );

    let deleted = client
        .delete(DeleteRequest { key: meta.key })
        .await
        .unwrap()
        .into_inner();
    assert!(deleted.deleted);
}

#[tokio::test]
async fn identical_content_deduplicates_across_uploads() {
    let (mut client, _cluster, _guard) = mounted().await;
    let data = b"exactly the same bytes";

    let first = client
        .upload(tokio_stream::iter(upload_messages("a.txt", data)))
        .await
        .unwrap()
        .into_inner();
    let second = client
        .upload(tokio_stream::iter(upload_messages("b.txt", data)))
        .await
        .unwrap()
        .into_inner();

    assert_eq!(first.key, second.key, "the key is the content");
    assert!(!first.deduplicated);
    assert!(second.deduplicated, "the second upload wrote nothing");
}

#[tokio::test]
async fn a_range_download_returns_only_that_range() {
    let (mut client, _cluster, _guard) = mounted().await;
    let data = b"0123456789abcdef";

    let meta = client
        .upload(tokio_stream::iter(upload_messages("d.bin", data)))
        .await
        .unwrap()
        .into_inner();

    let mut download = client
        .download(DownloadRequest {
            key: meta.key,
            range_start: Some(4),
            range_end: Some(7),
        })
        .await
        .unwrap()
        .into_inner();

    let mut out = Vec::new();
    while let Some(chunk) = download.next().await {
        out.extend_from_slice(&chunk.unwrap().data);
    }
    assert_eq!(out, b"4567");
}

#[tokio::test]
async fn an_upload_whose_first_message_is_not_info_is_refused() {
    let (mut client, _cluster, _guard) = mounted().await;
    let messages = vec![upload_chunk(Bytes::from_static(b"straight to content"))];

    let err = client
        .upload(tokio_stream::iter(messages))
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::InvalidArgument);
}

#[tokio::test]
async fn downloading_an_unknown_key_is_not_found() {
    let (mut client, _cluster, _guard) = mounted().await;
    let err = client
        .download(DownloadRequest {
            key: "files/nope".to_owned(),
            range_start: None,
            range_end: None,
        })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);
}

#[tokio::test]
async fn a_deleted_file_is_gone_from_stat() {
    let (mut client, _cluster, _guard) = mounted().await;
    let meta = client
        .upload(tokio_stream::iter(upload_messages("x", b"content")))
        .await
        .unwrap()
        .into_inner();

    client
        .delete(DeleteRequest {
            key: meta.key.clone(),
        })
        .await
        .unwrap();
    let err = client
        .stat(StatRequest { key: meta.key })
        .await
        .unwrap_err();
    assert_eq!(err.code(), tonic::Code::NotFound);
}
