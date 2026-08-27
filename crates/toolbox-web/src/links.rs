//! RFC 8288 `Link` headers for pagination.
//!
//! RFC 8288 already defines how to express "the next page", and a client that
//! understands it needs no knowledge of your query parameters. The body keeps
//! carrying the counts; the header carries the navigation.

use http::HeaderValue;
use toolbox_core::{Page, PageRequest};

/// Build the `Link` header value for a page.
///
/// Returns `None` for an unpaged result, which has no navigation.
///
/// `base` is the request path including any filters that must be preserved,
/// without the paging parameters - those are appended here.
///
/// # Arguments
///
/// * `page` - The page to build navigation for. An unpaged one has none, and
///   gives `None`.
/// * `base` - The request path including any filters that must be preserved,
///   without the paging parameters, which are appended here.
#[must_use]
pub fn page_links<T>(page: &Page<T>, base: &str) -> Option<HeaderValue> {
    let PageRequest::Paged {
        offset,
        limit,
        sort,
    } = page.request()
    else {
        return None;
    };

    let total = page.total();
    let sort = if sort.is_empty() {
        String::new()
    } else {
        format!("&sort={sort}")
    };
    let mut links = Vec::new();

    let url = |offset: i64| format!("{base}?offset={offset}&limit={limit}{sort}");

    if *offset > 0 {
        links.push(format!("<{}>; rel=\"first\"", url(0)));
        links.push(format!("<{}>; rel=\"prev\"", url((offset - limit).max(0))));
    }
    if offset + limit < total {
        links.push(format!("<{}>; rel=\"next\"", url(offset + limit)));
    }
    if total > 0 {
        // The offset of the final page, which is the largest multiple of the
        // limit strictly below the total.
        let last = ((total - 1) / limit) * limit;
        links.push(format!("<{}>; rel=\"last\"", url(last)));
    }

    if links.is_empty() {
        return None;
    }
    HeaderValue::from_str(&links.join(", ")).ok()
}

/// Attach `Link` and the pagination count headers to a response.
///
/// `X-Total-Count` is not a standard, and is included because every frontend
/// table wants it and reading it from a header is cheaper than parsing the
/// body twice. `Link` is the part a generic client uses.
///
/// # Arguments
///
/// * `headers` - The response headers to write into.
/// * `page` - The page whose navigation and total to advertise.
/// * `base` - The request path, as for [`page_links`].
pub fn attach_page_headers<T>(headers: &mut http::HeaderMap, page: &Page<T>, base: &str) {
    if let Some(links) = page_links(page, base) {
        headers.insert(http::header::LINK, links);
    }
    if let Ok(total) = HeaderValue::from_str(&page.total().to_string()) {
        headers.insert("x-total-count", total);
    }
}
