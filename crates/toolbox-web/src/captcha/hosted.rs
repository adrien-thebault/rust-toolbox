//! Verification against one of the hosted providers.

use std::time::Duration;

use async_trait::async_trait;
use serde::Deserialize;
use tracing::{debug, warn};

use super::{CaptchaProvider, CaptchaVerifier};
use crate::error::ApiError;

/// How long to wait for the provider before giving up.
///
/// Short: a captcha provider being slow must not become your login endpoint
/// being slow.
const TIMEOUT: Duration = Duration::from_secs(5);

#[derive(Debug, Deserialize)]
struct SiteVerify {
    success: bool,
    #[serde(default, rename = "error-codes")]
    error_codes: Vec<String>,
}

/// Verifies against one of the three hosted providers.
pub struct HostedCaptcha {
    provider: CaptchaProvider,
    secret: secrecy::SecretString,
    http: reqwest::Client,
}

impl std::fmt::Debug for HostedCaptcha {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HostedCaptcha")
            .field("provider", &self.provider)
            .finish_non_exhaustive()
    }
}

impl HostedCaptcha {
    /// Build a verifier.
    ///
    /// # Arguments
    ///
    /// * `provider` - Which hosted service to verify against.
    /// * `secret` - The provider's server-side secret. It never reaches the
    ///   browser.
    ///
    /// # Errors
    /// [`ApiError`] when the HTTP client cannot be built.
    pub fn new(provider: CaptchaProvider, secret: impl Into<String>) -> Result<Self, ApiError> {
        let http = reqwest::ClientBuilder::new()
            .timeout(TIMEOUT)
            .build()
            .map_err(ApiError::internal)?;
        Ok(Self {
            provider,
            secret: secrecy::SecretString::from(secret.into()),
            http,
        })
    }
}

#[async_trait]
impl CaptchaVerifier for HostedCaptcha {
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
            .post(self.provider.endpoint())
            .form(&form)
            .send()
            .await
            .map_err(|e| {
                // Unreachable is not the same as invalid. Returning "invalid"
                // here would lock every user out when the provider has an
                // outage; returning an error lets the caller decide.
                warn!(error = %e, "the captcha provider could not be reached");
                ApiError::of_kind(toolbox_core::ErrorKind::Unavailable, "Service Unavailable")
                    .with_code("CAPTCHA_UNAVAILABLE")
            })?;

        let verified: SiteVerify = response.json().await.map_err(|e| {
            ApiError::of_kind(toolbox_core::ErrorKind::Unavailable, "Service Unavailable")
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
