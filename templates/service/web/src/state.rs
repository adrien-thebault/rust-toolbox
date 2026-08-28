//! What the gateway holds, and what the auth routes read out of it.

use std::sync::Arc;

use toolbox_auth::{ProviderRegistry, RefreshTokens, SessionCodec};
use toolbox_grpc::BackendChannel;
use toolbox_web::auth::AuthState;

/// Everything a handler can reach.
#[derive(Clone)]
pub struct AppState {
    /// A channel to the backend.
    pub todos: BackendChannel,
    /// Everything a caller may log in with.
    pub providers: Arc<ProviderRegistry>,
    /// How sessions are issued and verified.
    pub sessions: Arc<SessionCodec>,
    /// Refresh tokens, when the deployment issues them.
    pub refresh: Option<Arc<RefreshTokens>>,
}

/// The three accessors `auth_router` needs. Implementing this is what mounts
/// login, refresh, logout, `/auth/me` and `/auth/providers` without writing any
/// of them.
impl AuthState for AppState {
    fn providers(&self) -> &ProviderRegistry {
        &self.providers
    }

    fn sessions(&self) -> &SessionCodec {
        &self.sessions
    }

    fn refresh_tokens(&self) -> Option<&Arc<RefreshTokens>> {
        self.refresh.as_ref()
    }
}
