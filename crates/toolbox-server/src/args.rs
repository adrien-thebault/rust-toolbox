//! The standard command-line arguments every binary takes.
//!
//! `DatabaseArgs` ships with `toolbox-db` and `BackendArgs` with
//! `toolbox-grpc`, next to the types they configure: putting an argument
//! struct one crate away from its type means every consumer writes the same
//! four-line bridge.

#[cfg(feature = "clap")]
use std::net::SocketAddr;

#[cfg(feature = "clap")]
use toolbox_cluster::Deployment;

/// Where to listen.
#[cfg(feature = "clap")]
#[derive(Debug, Clone, clap::Args)]
pub struct ServerArgs {
    /// The address to bind.
    #[arg(long, env = "LISTEN_ADDR", default_value = "0.0.0.0:8080")]
    pub listen_addr: SocketAddr,
}

/// How many replicas are running, and which one this is.
#[cfg(feature = "clap")]
#[derive(Debug, Clone, clap::Args)]
pub struct DeploymentArgs {
    /// `single` or `clustered`. Anything holding state in process memory is
    /// checked against this at startup.
    #[arg(long, env = "DEPLOYMENT", default_value = "single")]
    pub deployment: String,

    /// Identifies this replica in logs and lock ownership. Defaults to a
    /// generated id when clustered.
    #[arg(long, env = "INSTANCE_ID")]
    pub instance_id: Option<String>,
}

#[cfg(feature = "clap")]
impl DeploymentArgs {
    /// Resolve to a [`Deployment`].
    ///
    /// # Errors
    /// [`ArgsError::UnknownDeployment`] when the value is neither `single` nor
    /// `clustered`. Guessing here would defeat the guard entirely.
    pub fn resolve(&self) -> Result<Deployment, ArgsError> {
        match self.deployment.to_ascii_lowercase().as_str() {
            "single" => Ok(Deployment::Single),
            "clustered" => Ok(Deployment::Clustered {
                instance_id: self
                    .instance_id
                    .clone()
                    .unwrap_or_else(|| uuid::Uuid::now_v7().to_string()),
            }),
            other => Err(ArgsError::UnknownDeployment(other.to_owned())),
        }
    }
}

/// Why an argument could not be resolved.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ArgsError {
    /// `DEPLOYMENT` was neither `single` nor `clustered`.
    #[error("unknown deployment `{0}`; expected `single` or `clustered`")]
    UnknownDeployment(String),
}
