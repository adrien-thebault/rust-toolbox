//! Captcha verification for login and signup forms.
//!
//! Three providers with three slightly different response shapes behind one
//! trait, so swapping Turnstile for hCaptcha is a configuration change. The
//! verification itself is one POST - which is exactly why a naive version was
//! 44 lines and 100% generic.

mod always_pass;
mod hosted;

pub use always_pass::AlwaysPass;
use async_trait::async_trait;
pub use hosted::HostedCaptcha;

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

/// Which provider to verify against.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CaptchaProvider {
    /// Cloudflare Turnstile.
    Turnstile,
    /// hCaptcha.
    HCaptcha,
    /// Google reCAPTCHA v2/v3.
    ReCaptcha,
}

impl CaptchaProvider {
    /// The verification endpoint.
    #[must_use]
    pub fn endpoint(self) -> &'static str {
        match self {
            Self::Turnstile => "https://challenges.cloudflare.com/turnstile/v0/siteverify",
            Self::HCaptcha => "https://api.hcaptcha.com/siteverify",
            Self::ReCaptcha => "https://www.google.com/recaptcha/api/siteverify",
        }
    }
}
