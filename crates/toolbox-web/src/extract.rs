//! Extractors that put a check in the handler's signature rather than its body.

pub mod auth;
pub mod idempotent;
pub mod page;
pub mod valid;

pub use auth::{Authenticated, MaybeAuthenticated};
pub use idempotent::{IDEMPOTENCY_KEY, IdempotencyKey, Idempotent, idempotency_key_max_len};
pub use page::PageQuery;
pub use valid::{ValidJson, ValidQuery};
