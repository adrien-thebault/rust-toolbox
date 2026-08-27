//! Pagination on the wire.
//!
//! The ~90-line block converting between a proto page message and the domain's
//! own page type was byte-identical in every consumer. It lives here once, and
//! consumers import `toolbox/v1/pagination.proto` instead of copying it.

use toolbox_core::{Page, PageRequest, Sort};

pub use crate::{
    proto,
    proto::{PageInfo, PageRequest as PageRequestProto},
};

/// Where the toolbox's protos live, for a consumer's `tonic-build` include
/// path.
///
/// ```ignore
/// tonic_prost_build::configure()
///     .compile_protos(&["proto/mine.proto"], &["proto", toolbox_grpc::PROTO_INCLUDE])?;
/// ```
pub const PROTO_INCLUDE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/proto");

impl From<&PageRequest> for PageRequestProto {
    fn from(request: &PageRequest) -> Self {
        Self {
            offset: request.offset().unwrap_or(0),
            // Zero means unpaged on the wire, which is also proto3's default,
            // so an unset field and an unpaged request agree.
            limit: request.limit().unwrap_or(0),
            sort: request.sort().to_string(),
        }
    }
}

impl PageRequestProto {
    /// Validate this into a domain [`PageRequest`].
    ///
    /// # Errors
    /// [`toolbox_core::PageError`] when the offset is negative, the limit is
    /// negative, or the sort does not parse. A zero limit is unpaged, not an
    /// error.
    pub fn to_domain(&self) -> Result<PageRequest, toolbox_core::PageError> {
        let sort = Sort::parse(&self.sort)?;
        if self.limit == 0 {
            return Ok(PageRequest::unpaged(sort));
        }
        PageRequest::paged(self.offset, self.limit, sort)
    }
}

impl<T> From<&Page<T>> for PageInfo {
    fn from(page: &Page<T>) -> Self {
        Self {
            offset: page.request().offset().unwrap_or(0),
            limit: page.request().limit().unwrap_or(0),
            total: page.total(),
            sort: page.request().sort().to_string(),
        }
    }
}

impl PageInfo {
    /// Rebuild the page metadata this describes.
    ///
    /// # Errors
    /// [`toolbox_core::PageError`] when the values do not form a valid window.
    pub fn to_request(&self) -> Result<PageRequest, toolbox_core::PageError> {
        let sort = Sort::parse(&self.sort)?;
        if self.limit == 0 {
            return Ok(PageRequest::unpaged(sort));
        }
        PageRequest::paged(self.offset, self.limit, sort)
    }
}

/// Split a page into the two halves a `ListXResponse` carries.
///
/// This plus [`toolbox_core::Page::try_map`] is the whole conversion that used
/// to be written out per handler:
///
/// ```ignore
/// let (items, page_info) = split(page.try_map(Event::try_into)?);
/// Ok(Response::new(ListEventsResponse { items, page_info: Some(page_info) }))
/// ```
///
/// # Arguments
///
/// * `page` - The page to take apart. Its rows become the response's repeated
///   field and its window becomes the `PageInfo`.
#[must_use]
pub fn split<T>(page: Page<T>) -> (Vec<T>, PageInfo) {
    let info = PageInfo::from(&page);
    (page.into_items(), info)
}
