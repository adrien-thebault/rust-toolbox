//! Transport-neutral error vocabulary shared by every other toolbox crate.
//!
//! It unifies how an error is classified and reported across the HTTP, gRPC
//! and database boundaries, so a value crossing two of them is not translated
//! twice.
//!
//! At its core this crate depends on `serde` and nothing else. Anything that
//! would add a runtime dependency belongs one layer up: `CloudEvent` needs
//! `uuid`, so it lives in `toolbox-cluster`. The one opt-in is the `derive`
//! feature, which adds the `#[derive(ServiceError)]` proc macro (a build-time
//! dependency, not a runtime one).

pub mod error;
pub mod problem;

pub use error::{ErrorInfo, ErrorKind, ServiceError};
pub use problem::{ABOUT_BLANK, PROBLEM_JSON, Problem};
/// The derive. Shares its name with the [`ServiceError`] trait it implements,
/// the way `serde::Serialize` does. Behind the `derive` feature.
#[cfg(feature = "derive")]
pub use toolbox_macros::ServiceError;
