//! Captcha verification for login and signup forms.
//!
//! One trait, so a login route depends on "something that checks a token"
//! rather than a specific vendor.

mod always_pass;
mod third_party;

pub use always_pass::AlwaysPass;
use async_trait::async_trait;
pub use third_party::ThirdPartyCaptcha;

use crate::error::ApiError;

/// Checks a captcha token.
#[async_trait]
pub trait CaptchaVerifier: Send + Sync + 'static {
    /// Whether the token is good.
    ///
    /// `remote_ip` is optional and improves the provider's scoring; pass the
    /// result of `client_ip`, so the captcha and the rate limiter agree about
    /// who the caller is.
    ///
    /// # Arguments
    ///
    /// * `token` - What the widget produced in the browser.
    /// * `remote_ip` - The caller's address, which improves the provider's
    ///   scoring. Pass the result of `client_ip`, so the captcha and the rate
    ///   limiter agree on who the caller is.
    ///
    /// # Errors
    /// [`ApiError`] when the provider could not be reached, which is
    /// deliberately **not** the same as the token being bad.
    async fn verify(&self, token: &str, remote_ip: Option<&str>) -> Result<bool, ApiError>;
}
