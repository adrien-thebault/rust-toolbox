//! What a consumer plugs into the file service: who may act, and what to
//! do afterwards.

use tonic::metadata::MetadataMap;

/// Decides whether a caller may touch a file.
///
/// The default permits everything, and that is correct for this architecture:
/// the gateway is the auth layer, and this service assumes its caller is
/// already trusted. Which is exactly why service auth on the channel matters -
/// see `toolbox_grpc::require_service_auth`.
pub trait AuthorizeFile: Send + Sync + 'static {
    /// Whether this request may proceed.
    ///
    /// # Arguments
    ///
    /// * `key` - The file being reached, or `None` for an upload, where there
    ///   is no key yet.
    /// * `request` - The request metadata, which is where a caller's credential
    ///   would be.
    fn authorize(&self, key: Option<&str>, request: &MetadataMap) -> bool;
}

/// Permits everything.
#[derive(Debug, Clone, Copy, Default)]
pub struct PermitAll;

impl AuthorizeFile for PermitAll {
    fn authorize(&self, _key: Option<&str>, _request: &MetadataMap) -> bool {
        true
    }
}

/// Told when a file is stored or removed, so a consumer can react without
/// this service knowing what the reaction is.
pub trait FileEventHook: Send + Sync + 'static {
    /// A file was stored. `deduplicated` means nothing new was written.
    ///
    /// # Arguments
    ///
    /// * `meta` - The file's identity, so a consumer can index or post-process
    ///   it.
    /// * `deduplicated` - Whether the content was already there. `true` means
    ///   no bytes were written, which a thumbnailer should skip.
    fn stored(&self, meta: &crate::FileMeta, deduplicated: bool);
    /// A file was removed.
    ///
    /// # Arguments
    ///
    /// * `key` - The file that was removed, so a consumer can drop what it
    ///   derived from it.
    fn deleted(&self, key: &str);
}

/// Does nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoHooks;

impl FileEventHook for NoHooks {
    fn stored(&self, _meta: &crate::FileMeta, _deduplicated: bool) {}
    fn deleted(&self, _key: &str) {}
}
