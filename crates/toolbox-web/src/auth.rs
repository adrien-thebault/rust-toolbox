//! The login routes and the session middleware.
//!
//! Login, refresh, logout and "who am I" are the same four endpoints in every
//! project, and two of them have a security-relevant detail that is easy to get
//! wrong:
//!
//! - **The credential rate limit is attached in [`auth_router`]**, to
//!   `/auth/login` and `/auth/refresh` and to nothing else. Wiring it by hand
//!   means reasoning about axum's "a layer only wraps routes already added"
//!   ordering rule every time, and getting it wrong throttles `/auth/me` on
//!   every page load.
//! - **Refresh tokens are stateless JWTs.** A short access token with no
//!   refresh logs the user out constantly; a long one is a revocation window
//!   nobody wants. The refresh token carries the principal and, optionally, a
//!   fingerprint of the stored credential so "change your password" revokes it
//!   ([`AuthState::refresh_epoch`]).

mod forwarded;
mod routes;
mod session;
mod state;

pub use forwarded::{ForwardedConfig, forwarded_auth_layer};
pub use routes::{LoginRequest, RefreshRequest, SessionResponse, auth_router};
pub use session::session_layer;
pub use state::AuthState;
