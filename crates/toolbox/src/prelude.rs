//! The handful of names almost every file needs.
//!
//! Deliberately small. A prelude that pulls in fifty names makes every
//! unqualified identifier ambiguous to a reader.

#[cfg(feature = "auth")]
pub use toolbox_auth::{Principal, Role};
#[cfg(feature = "cluster")]
pub use toolbox_cluster::{CloudEvent, Deployment};
#[cfg(feature = "core")]
pub use toolbox_core::{ErrorKind, Page, PageRequest, Problem, ServiceError, Sort};
#[cfg(feature = "db")]
pub use toolbox_db::{Db, DbError, DbResult, Entity, Paginate};
#[cfg(feature = "grpc")]
pub use toolbox_grpc::{GrpcResult, to_status};
#[cfg(feature = "web")]
pub use toolbox_web::{ApiError, Authenticated, PageQuery, ValidJson};
