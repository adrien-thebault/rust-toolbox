//! Who this domain trusts, and what it lets them do.
//!
//! The gateway resolves who the end user is; this domain only decides what an
//! already-resolved caller may do to *its* data. It never sees a password or a
//! bearer token - only the [`toolbox_auth::AssertedPrincipal`] the gateway
//! attached, gated by [`toolbox_grpc::server::shared_secret::shared_secret_layer`].

use toolbox_auth::Role;

/// The one role this domain checks.
///
/// Its own copy rather than a shared crate: two independently deployed
/// services agree on the role *name* ("ADMIN"), not on a Rust type across a
/// process boundary.
pub struct Admin;

impl Role for Admin {
    const NAME: &'static str = "ADMIN";
}
