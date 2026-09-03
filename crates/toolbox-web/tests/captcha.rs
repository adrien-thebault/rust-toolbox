//! `CaptchaProvider` endpoint tests; the verifiers are in the submodules.

use toolbox_web::captcha::CaptchaProvider;

mod always_pass;
mod third_party;

#[test]
fn each_provider_has_its_own_endpoint() {
    assert!(
        CaptchaProvider::Turnstile
            .endpoint()
            .contains("cloudflare.com")
    );
    assert!(
        CaptchaProvider::HCaptcha
            .endpoint()
            .contains("hcaptcha.com")
    );
    assert!(CaptchaProvider::ReCaptcha.endpoint().contains("google.com"));
}

#[test]
fn every_endpoint_is_https() {
    for provider in [
        CaptchaProvider::Turnstile,
        CaptchaProvider::HCaptcha,
        CaptchaProvider::ReCaptcha,
    ] {
        assert!(provider.endpoint().starts_with("https://"), "{provider:?}");
    }
}
