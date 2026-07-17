use crate::diesel_tools::sort::Sort;
use std::cmp;
use thiserror::Error;

/// paginated data
#[derive(Clone, Debug)]
pub struct Page<T> {
    /// the actual data
    pub data: Vec<T>,

    /// the pagination request
    pub page_request: PageRequest,

    /// the total number of elements that would have been returned
    /// if the request wasn't paginated
    pub total_elements: i64,
}

impl<T> Page<T> {
    /// returns the size of the page
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// checks if the page is empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// maps the content of the page
    pub fn map<U>(self, map: impl FnMut(T) -> U) -> Page<U> {
        Page {
            data: self.data.into_iter().map(map).collect(),
            page_request: self.page_request,
            total_elements: self.total_elements,
        }
    }

    /// filter/maps the content of the page
    pub fn filter_map<U>(self, filter_map: impl FnMut(T) -> Option<U>) -> Page<U> {
        Page {
            data: self.data.into_iter().filter_map(filter_map).collect(),
            page_request: self.page_request,
            total_elements: self.total_elements,
        }
    }

    /// the next page request, or `None` if this page is already the last one
    /// (or the request was unpaged). Unlike
    /// [`PageRequest::next_page`](PageRequest::next_page), this is bounds-checked
    /// against [`total_elements`](Self::total_elements) - `PageRequest` alone
    /// doesn't know how many elements exist, so it can't tell.
    pub fn next_page(&self) -> Option<PageRequest> {
        match &self.page_request {
            PageRequest::Paged { offset, size, .. } if offset + size < self.total_elements => {
                self.page_request.next_page()
            }
            _ => None,
        }
    }

    /// the previous page request, or `None` if this page is already the
    /// first one (or the request was unpaged). Bounds-checked the same way
    /// as [`next_page`](Self::next_page).
    pub fn previous_page(&self) -> Option<PageRequest> {
        match &self.page_request {
            PageRequest::Paged { offset, .. } if *offset > 0 => self.page_request.previous_page(),
            _ => None,
        }
    }
}

/// pagination data
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PageRequest {
    /// paged request
    Paged {
        /// where to start
        offset: i64,
        /// how many elements we want to retrieve
        size: i64,
        /// sort data
        sort: Sort,
    },

    /// unpaged request
    Unpaged {
        /// sort data
        sort: Sort,
    },
}

impl PageRequest {
    /// builds a validated [`PageRequest::Paged`]: `size` must be > 0 and
    /// `offset` >= 0. Prefer this over constructing the variant directly
    /// when the values come from an untrusted caller -
    /// [`apply_page_request`](super::Repository::apply_page_request) also
    /// clamps defensively, but rejecting bad input up front gives the
    /// caller a proper error instead of an empty page.
    pub fn paged(offset: i64, size: i64, sort: Sort) -> Result<Self, PaginationError> {
        if size <= 0 {
            return Err(PaginationError::InvalidPageSize);
        }
        if offset < 0 {
            return Err(PaginationError::InvalidOffset);
        }
        Ok(Self::Paged { offset, size, sort })
    }

    /// computes the next logical page request if it exists
    pub fn next_page(&self) -> Option<Self> {
        match self {
            Self::Unpaged { .. } => None,
            Self::Paged { sort, size, offset } => Some(Self::Paged {
                sort: sort.clone(),
                size: *size,
                offset: offset + size,
            }),
        }
    }

    /// computes the previous page request if it exists
    pub fn previous_page(&self) -> Option<Self> {
        match self {
            Self::Unpaged { .. } => None,
            Self::Paged { sort, size, offset } => Some(Self::Paged {
                sort: sort.clone(),
                size: *size,
                offset: cmp::max(0, offset - size),
            }),
        }
    }
}

impl Default for PageRequest {
    fn default() -> Self {
        Self::Unpaged {
            sort: Sort::default(),
        }
    }
}

/// errors from [`PageRequest::paged`]
#[derive(Error, Debug, PartialEq, Eq)]
pub enum PaginationError {
    /// the provided page request had an invalid page size
    #[error("page size must be > 0")]
    InvalidPageSize,

    /// the provided page request had an invalid offset
    #[error("page offset must be >= 0")]
    InvalidOffset,
}

#[cfg(test)]
mod tests {
    use crate::diesel_tools::{
        pagination::{Page, PageRequest},
        sort::{Sort, SortDirection},
    };

    type Result = std::result::Result<(), Box<dyn std::error::Error>>;

    // test Page::next_page/previous_page (bounds-checked against total_elements,
    // unlike the bare PageRequest methods above)
    #[test]
    fn page_next_and_previous_page_are_bounds_checked() {
        // first page of three: no previous page, a next page exists
        let page = Page {
            data: Vec::<()>::new(),
            page_request: PageRequest::Paged {
                offset: 0,
                size: 10,
                sort: Sort::Unsorted,
            },
            total_elements: 25,
        };
        assert!(page.previous_page().is_none());
        assert_eq!(
            page.next_page(),
            Some(PageRequest::Paged {
                offset: 10,
                size: 10,
                sort: Sort::Unsorted
            })
        );

        // middle page: both exist
        let page = Page {
            data: Vec::<()>::new(),
            page_request: PageRequest::Paged {
                offset: 10,
                size: 10,
                sort: Sort::Unsorted,
            },
            total_elements: 25,
        };
        assert_eq!(
            page.previous_page(),
            Some(PageRequest::Paged {
                offset: 0,
                size: 10,
                sort: Sort::Unsorted
            })
        );
        assert_eq!(
            page.next_page(),
            Some(PageRequest::Paged {
                offset: 20,
                size: 10,
                sort: Sort::Unsorted
            })
        );

        // last page: a previous page exists, no next page
        let page = Page {
            data: Vec::<()>::new(),
            page_request: PageRequest::Paged {
                offset: 20,
                size: 10,
                sort: Sort::Unsorted,
            },
            total_elements: 25,
        };
        assert!(page.previous_page().is_some());
        assert!(page.next_page().is_none());

        // unpaged: neither exists
        let page = Page {
            data: Vec::<()>::new(),
            page_request: PageRequest::Unpaged {
                sort: Sort::Unsorted,
            },
            total_elements: 25,
        };
        assert!(page.previous_page().is_none());
        assert!(page.next_page().is_none());
    }

    // test next_page
    #[test]
    fn next_page() -> Result {
        // paged + sorted
        let page_request = PageRequest::Paged {
            offset: 138,
            size: 69,
            sort: Sort::Sorted {
                items: vec![
                    ("fieldA".to_string(), SortDirection::Asc),
                    ("fieldB".to_string(), SortDirection::Desc),
                ],
            },
        };

        assert_eq!(
            page_request.next_page(),
            Some(PageRequest::Paged {
                offset: 207,
                size: 69,
                sort: Sort::Sorted {
                    items: vec![
                        ("fieldA".to_string(), SortDirection::Asc),
                        ("fieldB".to_string(), SortDirection::Desc),
                    ],
                },
            })
        );

        // unpaged + unsorted
        let page_request = PageRequest::Unpaged {
            sort: Sort::Unsorted,
        };

        assert!(page_request.next_page().is_none());

        Ok(())
    }

    // test previous_page
    #[test]
    fn previous_page() -> Result {
        // paged + sorted
        let page_request = PageRequest::Paged {
            offset: 138,
            size: 69,
            sort: Sort::Sorted {
                items: vec![
                    ("fieldA".to_string(), SortDirection::Asc),
                    ("fieldB".to_string(), SortDirection::Desc),
                ],
            },
        };

        assert_eq!(
            page_request.previous_page(),
            Some(PageRequest::Paged {
                offset: 69,
                size: 69,
                sort: Sort::Sorted {
                    items: vec![
                        ("fieldA".to_string(), SortDirection::Asc),
                        ("fieldB".to_string(), SortDirection::Desc),
                    ],
                },
            })
        );

        // unpaged + unsorted
        let page_request = PageRequest::Unpaged {
            sort: Sort::Unsorted,
        };

        assert!(page_request.previous_page().is_none());

        Ok(())
    }

    // test default value
    #[test]
    fn try_default() -> Result {
        assert_eq!(
            PageRequest::default(),
            PageRequest::Unpaged {
                sort: Sort::Unsorted
            }
        );

        Ok(())
    }

    #[test]
    fn paged_constructor_validates_its_inputs() {
        use crate::diesel_tools::pagination::PaginationError;

        assert_eq!(
            PageRequest::paged(0, 10, Sort::Unsorted),
            Ok(PageRequest::Paged {
                offset: 0,
                size: 10,
                sort: Sort::Unsorted
            })
        );
        assert_eq!(
            PageRequest::paged(0, 0, Sort::Unsorted),
            Err(PaginationError::InvalidPageSize)
        );
        assert_eq!(
            PageRequest::paged(0, -1, Sort::Unsorted),
            Err(PaginationError::InvalidPageSize)
        );
        assert_eq!(
            PageRequest::paged(-1, 10, Sort::Unsorted),
            Err(PaginationError::InvalidOffset)
        );
    }
}
