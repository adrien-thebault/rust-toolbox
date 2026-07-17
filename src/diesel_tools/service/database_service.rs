use crate::diesel_tools::DatabasePool;

/// common shape of a gRPC service backed by a database pool: constructible
/// from a pool, and convertible into its tonic-generated server wrapper.
///
/// implemented by every `*Service` struct in a consuming crate, alongside
/// [`EntityService`](crate::diesel_tools::EntityService) for the actual
/// request-handling logic.
pub trait DatabaseService {
    /// the tonic-generated server wrapper for this service
    type Server;

    /// builds a new instance backed by the given pool
    fn new(pool: DatabasePool) -> Self;

    /// wraps this service into its tonic server type, ready for
    /// `tonic::transport::Server::add_service`
    fn into_server(self) -> Self::Server;
}
