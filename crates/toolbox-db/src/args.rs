//! The standard database command-line arguments.
//!
//! Lives here rather than in `toolbox-server` (which is where the other
//! argument structs are) so that `DbBuilder::from_args` can exist: putting the
//! struct one crate away meant every consumer wrote the same four-line bridge.

#[cfg(feature = "clap")]
use diesel::r2d2::R2D2Connection;

/// Where the database is and how large its pool may get.
#[cfg(feature = "clap")]
#[derive(Debug, Clone, clap::Args)]
pub struct DatabaseArgs {
    /// The connection URL.
    #[arg(long, env = "DATABASE_URL")]
    pub database_url: String,

    /// The largest number of pooled connections.
    #[arg(long, env = "DATABASE_MAX_CONNECTIONS", default_value_t = 8)]
    pub database_max_connections: u32,
}

#[cfg(feature = "clap")]
impl DatabaseArgs {
    /// A builder configured from these arguments.
    pub fn builder<C: R2D2Connection + 'static>(&self) -> crate::db::DbBuilder<C> {
        crate::db::Db::builder(self.database_url.clone()).max_size(self.database_max_connections)
    }
}
