use toolbox_web::captcha::{AlwaysPass, CaptchaProvider, CaptchaVerifier, HostedCaptcha};

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

#[tokio::test]
async fn the_development_verifier_accepts_everything() {
    assert!(AlwaysPass.verify("anything", None).await.unwrap());
    assert!(AlwaysPass.verify("", Some("203.0.113.9")).await.unwrap());
}

/// A Debug of a config struct ends up in a log.
#[test]
fn debug_never_prints_the_secret() {
    let verifier = HostedCaptcha::new(CaptchaProvider::Turnstile, "s3cret").unwrap();
    let rendered = format!("{verifier:?}");
    assert!(!rendered.contains("s3cret"), "{rendered}");
}
