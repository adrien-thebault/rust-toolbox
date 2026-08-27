//! A verifier that accepts everything.

use async_trait::async_trait;

use super::CaptchaVerifier;
use crate::error::ApiError;

/// Accepts everything, for a test or a development environment.
///
/// Named `AlwaysPass` rather than `NoCaptcha` so it is obvious in a wiring
/// diff that verification is off.
#[derive(Debug, Clone, Copy, Default)]
pub struct AlwaysPass;

#[async_trait]
impl CaptchaVerifier for AlwaysPass {
    async fn verify(&self, _token: &str, _remote_ip: Option<&str>) -> Result<bool, ApiError> {
        Ok(true)
    }
}
