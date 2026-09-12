//! Transport-neutral vocabulary shared by every other toolbox crate.
//!
//! It unifies the error, problem and pagination types across the HTTP, gRPC and
//! database boundaries, so a value crossing two of them is not translated
//! twice.
//!
//! At its core this crate depends on `serde` and `thiserror` and nothing else.
//! Anything that would add a runtime dependency belongs one layer up:
//! `CloudEvent` needs `uuid`, so it lives in `toolbox-cluster`, and no datetime
//! library appears here at all. The one opt-in is the `derive` feature, which
//! adds the `#[derive(ServiceError)]` proc macro (a build-time dependency, not
//! a runtime one).

pub mod error;
pub mod page;
pub mod problem;
pub mod sort;

pub use error::{ErrorInfo, ErrorKind, ServiceError};
pub use page::{MAX_LIMIT, Page, PageError, PageRequest};
pub use problem::{ABOUT_BLANK, PROBLEM_JSON, Problem};
pub use sort::{Sort, SortDirection, SortItem};
/// The derive. Shares its name with the [`ServiceError`] trait it implements,
/// the way `serde::Serialize` does. Behind the `derive` feature.
#[cfg(feature = "derive")]
pub use toolbox_macros::ServiceError;
