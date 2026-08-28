//! The mountable service.

use std::sync::Arc;

use bytes::Bytes;
use futures_util::StreamExt as _;
use object_store::ObjectStore;
use tonic::{Request, Response, Status, Streaming};

use crate::{
    Conditionals, UploadPolicy,
    proto::{
        Chunk, DeleteRequest, DeleteResponse, DownloadRequest, FileInfo,
        FileService as FileServiceTrait, FileServiceServer, StatRequest, UploadRequest,
        upload_request,
    },
    service::{
        hooks::{AuthorizeFile, FileEventHook, NoHooks, PermitAll},
        records::FileRecords,
    },
};

/// Builds the service.
pub struct FileServiceBuilder {
    records: Arc<dyn FileRecords>,
    store: Arc<dyn ObjectStore>,
    policy: UploadPolicy,
    authorize: Arc<dyn AuthorizeFile>,
    hook: Arc<dyn FileEventHook>,
}

impl FileServiceBuilder {
    /// Start from a record store and an object store.
    ///
    /// # Arguments
    ///
    /// * `records` - Where file identity is stored. The backend is the
    ///   consumer's, because this crate declares none.
    /// * `store` - Where the bytes go. Any `object_store` backend: local disk
    ///   in development, S3 in production.
    #[must_use]
    pub fn new(records: Arc<dyn FileRecords>, store: Arc<dyn ObjectStore>) -> Self {
        Self {
            records,
            store,
            policy: UploadPolicy::default(),
            authorize: Arc::new(PermitAll),
            hook: Arc::new(NoHooks),
        }
    }

    /// Set what may be uploaded.
    ///
    /// # Arguments
    ///
    /// * `policy` - The size cap and media-type allowlist for every upload this
    ///   service accepts.
    #[must_use]
    pub fn policy(mut self, policy: UploadPolicy) -> Self {
        self.policy = policy;
        self
    }

    /// Check callers. Defaults to permit-all; see [`AuthorizeFile`].
    ///
    /// # Arguments
    ///
    /// * `authorize` - The check to run before any file is touched. It defaults
    ///   to permit-all, which is correct when the gateway is the auth layer.
    #[must_use]
    pub fn authorize(mut self, authorize: impl AuthorizeFile) -> Self {
        self.authorize = Arc::new(authorize);
        self
    }

    /// React to stores and deletes.
    ///
    /// # Arguments
    ///
    /// * `hook` - What to call when a file is stored or deleted, so a consumer
    ///   can react without this service knowing what the reaction is.
    #[must_use]
    pub fn on_event(mut self, hook: impl FileEventHook) -> Self {
        self.hook = Arc::new(hook);
        self
    }

    /// The mountable tonic service.
    #[must_use]
    pub fn build(self) -> FileServiceServer<FileService> {
        FileServiceServer::new(FileService {
            records: self.records,
            store: self.store,
            policy: self.policy,
            authorize: self.authorize,
            hook: self.hook,
        })
    }
}

/// The service itself.
#[derive(Clone)]
pub struct FileService {
    records: Arc<dyn FileRecords>,
    store: Arc<dyn ObjectStore>,
    policy: UploadPolicy,
    authorize: Arc<dyn AuthorizeFile>,
    hook: Arc<dyn FileEventHook>,
}

/// A domain error as a gRPC status, kept local so this crate does not depend on
/// `toolbox-grpc` for its errors.
///
/// # Arguments
///
/// * `e` - The error to convert. Its kind decides the code; its message crosses
///   the wire.
fn to_status<E: toolbox_core::ServiceError>(e: E) -> Status {
    toolbox_grpc::to_status(e)
}

#[tonic::async_trait]
impl FileServiceTrait for FileService {
    async fn upload(
        &self,
        request: Request<Streaming<UploadRequest>>,
    ) -> Result<Response<FileInfo>, Status> {
        if !self.authorize.authorize(None, request.metadata()) {
            return Err(Status::permission_denied("not permitted to upload"));
        }

        let mut stream = request.into_inner();

        // The first message carries the name; everything after it is content.
        let filename = match stream.next().await {
            Some(Ok(UploadRequest {
                payload: Some(upload_request::Payload::Info(info)),
            })) => Some(info.filename).filter(|f| !f.is_empty()),
            Some(Ok(_)) => {
                return Err(Status::invalid_argument(
                    "the first message of an upload must carry UploadInfo",
                ));
            }
            Some(Err(e)) => return Err(e),
            None => return Err(Status::invalid_argument("an upload cannot be empty")),
        };

        let chunks = stream.map(|message| match message {
            Ok(UploadRequest {
                payload: Some(upload_request::Payload::Chunk(data)),
            }) => Ok(data),
            Ok(_) => Err(Status::invalid_argument("expected a chunk")),
            Err(e) => Err(e),
        });

        let ingested = crate::ingest_stream(self.store.as_ref(), chunks, &self.policy, filename)
            .await
            .map_err(to_status)?;

        self.records
            .record(&ingested.meta)
            .await
            .map_err(to_status)?;
        self.hook.stored(&ingested.meta, ingested.deduplicated);

        Ok(Response::new(FileInfo {
            key: ingested.meta.key,
            hash: ingested.meta.hash,
            filename: ingested.meta.filename.unwrap_or_default(),
            mime_type: ingested.meta.mime_type,
            size: ingested.meta.size,
            deduplicated: ingested.deduplicated,
        }))
    }

    type DownloadStream =
        std::pin::Pin<Box<dyn futures_core::Stream<Item = Result<Chunk, Status>> + Send>>;

    async fn download(
        &self,
        request: Request<DownloadRequest>,
    ) -> Result<Response<Self::DownloadStream>, Status> {
        let metadata = request.metadata().clone();
        let request = request.into_inner();
        if !self.authorize.authorize(Some(&request.key), &metadata) {
            return Err(Status::permission_denied("not permitted to read this file"));
        }

        let meta = self
            .records
            .get(&request.key)
            .await
            .map_err(to_status)?
            .ok_or_else(|| Status::not_found("no such file"))?;

        let range = match (request.range_start, request.range_end) {
            (Some(start), Some(end)) => Some(crate::ByteRange { start, end }),
            (Some(start), None) => Some(crate::ByteRange {
                start,
                end: meta.size.saturating_sub(1),
            }),
            _ => None,
        };

        // The same decision an HTTP download makes, so both agree on ranges.
        let decision = crate::serve(&meta, range, &Conditionals::default()).map_err(to_status)?;

        let body = crate::read_stream(Arc::clone(&self.store), &meta.key, decision.range)
            .await
            .map_err(to_status)?;

        let chunks = body.map(|chunk| chunk.map(|data: Bytes| Chunk { data }).map_err(to_status));

        Ok(Response::new(Box::pin(chunks)))
    }

    async fn stat(&self, request: Request<StatRequest>) -> Result<Response<FileInfo>, Status> {
        let metadata = request.metadata().clone();
        let key = request.into_inner().key;
        if !self.authorize.authorize(Some(&key), &metadata) {
            return Err(Status::permission_denied("not permitted to read this file"));
        }

        let meta = self
            .records
            .get(&key)
            .await
            .map_err(to_status)?
            .ok_or_else(|| Status::not_found("no such file"))?;

        Ok(Response::new(FileInfo {
            key: meta.key,
            hash: meta.hash,
            filename: meta.filename.unwrap_or_default(),
            mime_type: meta.mime_type,
            size: meta.size,
            deduplicated: false,
        }))
    }

    async fn delete(
        &self,
        request: Request<DeleteRequest>,
    ) -> Result<Response<DeleteResponse>, Status> {
        let metadata = request.metadata().clone();
        let key = request.into_inner().key;
        if !self.authorize.authorize(Some(&key), &metadata) {
            return Err(Status::permission_denied(
                "not permitted to delete this file",
            ));
        }

        // The record is marked deleted; the bytes stay. Content addressing
        // means another record may point at the same object, so removing it
        // here would delete somebody else's file.
        let deleted = self.records.delete(&key).await.map_err(to_status)?;
        if deleted {
            self.hook.deleted(&key);
        }
        Ok(Response::new(DeleteResponse { deleted }))
    }
}
