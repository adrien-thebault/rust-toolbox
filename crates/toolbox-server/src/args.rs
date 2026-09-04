//! The standard command-line arguments every binary takes.
//!
//! `DatabaseArgs` ships with `toolbox-db` and `BackendArgs` with
//! `toolbox-grpc`, next to the types they configure: putting an argument
//! struct one crate away from its type means every consumer writes the same
//! four-line bridge.

#[cfg(feature = "clap")]
use std::net::SocketAddr;

/// Where to listen.
#[cfg(feature = "clap")]
#[derive(Debug, Clone, clap::Args)]
pub struct ServerArgs {
    /// The address to bind.
    #[arg(long, env = "LISTEN_ADDR", default_value = "0.0.0.0:8080")]
    pub listen_addr: SocketAddr,
}
