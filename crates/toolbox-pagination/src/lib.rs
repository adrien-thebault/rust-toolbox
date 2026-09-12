//! One pagination and sort representation shared across every toolbox crate.
//!
//! It serves query strings, protobuf messages and SQL alike, encoding the
//! decisions - bounds validated at construction, overflow saturating, one
//! sort representation - that would otherwise be re-made per call site.
//!
//! This crate depends on `serde` and `thiserror` and nothing else, so it costs
//! nothing to pull in from a database entity, a gRPC message or an HTTP
//! extractor.

pub mod page;
pub mod sort;

pub use page::{MAX_LIMIT, Page, PageError, PageRequest};
pub use sort::{Sort, SortDirection, SortItem};
