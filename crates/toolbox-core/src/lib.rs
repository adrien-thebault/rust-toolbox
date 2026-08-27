//! Transport-neutral vocabulary shared by every other toolbox crate.
//!
//! It unifies the error, problem and pagination types across the HTTP, gRPC and
//! database boundaries, so a value crossing two of them is not translated
//! twice.
//!
//! This crate depends on `serde` and `thiserror` and nothing else. Anything
//! that would add a dependency belongs one layer up: `CloudEvent` needs `uuid`,
//! so it lives in `toolbox-cluster`, and no datetime library appears here at
//! all.

pub mod error;
pub mod page;
pub mod problem;

pub use error::{ErrorInfo, ErrorKind, ServiceError};
pub use page::{MAX_LIMIT, Page, PageError, PageRequest, Sort, SortDirection, SortItem};
pub use problem::{PROBLEM_JSON, Problem};
