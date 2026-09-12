//! Verification against a third-party siteverify endpoint.
//!
//! Turnstile, hCaptcha and reCAPTCHA all answer the same shape - `{success,
//! "error-codes"}` - because Turnstile and hCaptcha were both built as
//! drop-in reCAPTCHA replacements. So there is nothing here to select between:
//! the consumer names the endpoint, this posts the form and reads the answer.

use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use tracing::{debug, warn};

use super::CaptchaVerifier;
use crate::error::ApiError;

/// How long to wait for the provider before giving up.
///
/// Short: a captcha provider being slow must not become your login endpoint
/// being slow.
const TIMEOUT: Duration = Duration::from_secs(5);

/// The siteverify response shared by every provider that speaks this
/// protocol.
#[derive(Debug, Deserialize)]
struct SiteVerify {
    /// Whether the token passed.
    success: bool,
    /// Provider error codes, when it did not. `error-codes` is the vendors'
    /// field name on the wire, not ours.
    #[serde(default, rename = "error-codes")]
    error_codes: Vec<String>,
}

/// Verifies against a siteverify-shaped endpoint.
pub struct ThirdPartyCaptcha {
    /// The provider's verification endpoint.
    endpoint: String,
    /// The provider secret key.
    secret: secrecy::SecretString,
    /// The client used for the siteverify call.
    http: reqwest::Client,
}

impl std::fmt::Debug for ThirdPartyCaptcha {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ThirdPartyCaptcha")
            .field("endpoint", &self.endpoint)
            .finish_non_exhaustive()
    }
}

impl ThirdPartyCaptcha {
    /// Build a verifier.
    ///
    /// # Arguments
    ///
    /// * `endpoint` - The provider's siteverify URL - Turnstile's
    ///   `https://challenges.cloudflare.com/turnstile/v0/siteverify`,
    ///   hCaptcha's `https://api.hcaptcha.com/siteverify`, reCAPTCHA's
    ///   `https://www.google.com/recaptcha/api/siteverify`, or a self-hosted
    ///   one that speaks the same protocol.
    /// * `secret` - The provider's server-side secret. It never reaches the
    ///   browser.
    ///
    /// # Errors
    /// [`ApiError`] when the HTTP client cannot be built.
    pub fn new(endpoint: impl Into<String>, secret: impl Into<String>) -> Result<Self, ApiError> {
        let http = reqwest::ClientBuilder::new()
            .timeout(TIMEOUT)
            .build()
            .map_err(ApiError::internal)?;
        Ok(Self {
            endpoint: endpoint.into(),
            secret: secrecy::SecretString::from(secret.into()),
            http,
        })
    }
}

#[async_trait]
impl CaptchaVerifier for ThirdPartyCaptcha {
    async fn verify(&self, token: &str, remote_ip: Option<&str>) -> Result<bool, ApiError> {
        use secrecy::ExposeSecret as _;

        let mut form = vec![
            ("secret", self.secret.expose_secret().to_owned()),
            ("response", token.to_owned()),
        ];
        if let Some(ip) = remote_ip {
            form.push(("remoteip", ip.to_owned()));
        }

        let response = self
            .http
            .post(&self.endpoint)
            .form(&form)
            .send()
            .await
            .map_err(|e| {
                // Unreachable is not the same as invalid. Returning "invalid"
                // here would lock every user out when the provider has an
                // outage; returning an error lets the caller decide.
                warn!(error = %e, "the captcha provider could not be reached");
                ApiError::of_kind(toolbox_error::ErrorKind::Unavailable, "Service Unavailable")
                    .with_code("CAPTCHA_UNAVAILABLE")
            })?;

        let verified: SiteVerify = response.json().await.map_err(|e| {
            ApiError::of_kind(toolbox_error::ErrorKind::Unavailable, "Service Unavailable")
                .with_code("CAPTCHA_UNAVAILABLE")
                .with_source(e)
        })?;

        if !verified.success && !verified.error_codes.is_empty() {
            // The codes name configuration mistakes - a wrong secret, a
            // hostname mismatch - so they are worth logging but never worth
            // returning, since they describe your setup rather than the caller.
            debug!(codes = ?verified.error_codes, "captcha verification failed");
        }
        Ok(verified.success)
    }
}
