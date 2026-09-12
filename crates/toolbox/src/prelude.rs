//! The handful of names almost every file needs.
//!
//! Deliberately small. A prelude that pulls in fifty names makes every
//! unqualified identifier ambiguous to a reader.

#[cfg(feature = "auth")]
pub use toolbox_auth::{Principal, Role};
#[cfg(feature = "cluster")]
pub use toolbox_cluster::CloudEvent;
#[cfg(feature = "db")]
pub use toolbox_db::{Db, DbError, DbResult, Entity, Paginate};
#[cfg(feature = "error")]
pub use toolbox_error::{ErrorKind, Problem, ServiceError};
#[cfg(feature = "grpc")]
pub use toolbox_grpc::{GrpcResult, to_status};
#[cfg(feature = "pagination")]
pub use toolbox_pagination::{Page, PageRequest, Sort};
#[cfg(feature = "web")]
pub use toolbox_web::{ApiError, Authenticated, PageQuery, ValidJson};
