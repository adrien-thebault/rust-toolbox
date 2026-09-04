//! A cluster of gRPC backends on ephemeral ports.

use std::{collections::BTreeMap, net::SocketAddr, time::Duration};

use tonic::service::RoutesBuilder;

/// How long to wait for a spawned backend to accept a connection.
///
/// **Bounded, deliberately.** A naive harness busy-looped forever, so a
/// service that failed to bind hung the test until the whole run was killed -
/// and the output said nothing about which one.
const READY_TIMEOUT: Duration = Duration::from_secs(10);

/// How often to retry while waiting.
const POLL_INTERVAL: Duration = Duration::from_millis(20);

/// Why a cluster could not be brought up.
#[derive(Debug, thiserror::Error)]
#[non_exhaustive]
pub enum ClusterError {
    /// A backend could not bind a port.
    #[error("backend `{name}` could not bind: {source}")]
    Bind {
        /// Which backend.
        name: &'static str,
        /// Why.
        source: std::io::Error,
    },
    /// A backend bound but never started accepting within the timeout.
    #[error("backend `{name}` did not become ready within {timeout:?}")]
    NotReady {
        /// Which backend.
        name: &'static str,
        /// How long was waited.
        timeout: Duration,
    },
}

/// Several gRPC backends, each on its own ephemeral port.
#[derive(Debug, Default)]
pub struct TestCluster {
    /// The ephemeral address of each started backend, by name.
    addrs: BTreeMap<&'static str, SocketAddr>,
    /// The serve task for each backend, aborted on drop.
    handles: Vec<tokio::task::JoinHandle<()>>,
}

impl TestCluster {
    /// An empty cluster.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Spawn one backend, built by `build`, on an ephemeral port.
    ///
    /// Ephemeral rather than fixed: a fixed port makes two test binaries
    /// running at once fail in a way that looks like a flaky test.
    ///
    /// # Arguments
    ///
    /// * `name` - How the test will refer to this backend when asking for its
    ///   address.
    /// * `build` - Builds the tonic service, given the address it was assigned.
    ///   It runs after the port is known, which is what lets a backend know its
    ///   own address.
    ///
    /// # Errors
    /// [`ClusterError::Bind`] when the port cannot be bound, or
    /// [`ClusterError::NotReady`] when the server never starts accepting.
    pub async fn service<F>(mut self, name: &'static str, build: F) -> Result<Self, ClusterError>
    where
        F: FnOnce(&mut RoutesBuilder),
    {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .map_err(|source| ClusterError::Bind { name, source })?;
        let addr = listener
            .local_addr()
            .map_err(|source| ClusterError::Bind { name, source })?;

        let mut routes = RoutesBuilder::default();
        build(&mut routes);

        let handle = tokio::spawn(async move {
            let _ = tonic::transport::Server::builder()
                .add_routes(routes.routes())
                .serve_with_incoming(tokio_stream::wrappers::TcpListenerStream::new(listener))
                .await;
        });

        wait_until_accepting(name, addr).await?;
        self.addrs.insert(name, addr);
        self.handles.push(handle);
        Ok(self)
    }

    /// One backend's address, or `None` if nothing was registered under `name`.
    ///
    /// # Arguments
    ///
    /// * `name` - The backend's registered name.
    #[must_use]
    pub fn backend(&self, name: &str) -> Option<SocketAddr> {
        self.addrs.get(name).copied()
    }

    /// One backend's address as a `http://host:port` URI, to build a client from.
    ///
    /// # Arguments
    ///
    /// * `name` - The backend's registered name.
    ///
    /// # Panics
    /// When no backend was registered under `name`, which is a test wiring
    /// mistake worth failing on rather than feeding `None` into a client builder.
    #[must_use]
    pub fn backend_uri(&self, name: &str) -> String {
        let addr = self
            .backend(name)
            .unwrap_or_else(|| panic!("no backend named `{name}`; have {:?}", self.addrs.keys()));
        format!("http://{addr}")
    }
}

impl Drop for TestCluster {
    fn drop(&mut self) {
        // Ports go back immediately rather than when the runtime happens to
        // reap the tasks.
        for handle in &self.handles {
            handle.abort();
        }
    }
}

/// Poll until the address accepts a connection, or give up.
///
/// # Arguments
///
/// * `name` - The backend being waited for, named in the timeout error.
/// * `addr` - Where it should be listening.
async fn wait_until_accepting(name: &'static str, addr: SocketAddr) -> Result<(), ClusterError> {
    let deadline = tokio::time::Instant::now() + READY_TIMEOUT;
    loop {
        if tokio::net::TcpStream::connect(addr).await.is_ok() {
            return Ok(());
        }
        if tokio::time::Instant::now() >= deadline {
            return Err(ClusterError::NotReady {
                name,
                timeout: READY_TIMEOUT,
            });
        }
        tokio::time::sleep(POLL_INTERVAL).await;
    }
}
