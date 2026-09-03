use toolbox_web::captcha::{CaptchaProvider, ThirdPartyCaptcha};

/// A Debug of a config struct ends up in a log.
#[test]
fn debug_never_prints_the_secret() {
    let verifier = ThirdPartyCaptcha::new(CaptchaProvider::Turnstile, "s3cret").unwrap();
    let rendered = format!("{verifier:?}");
    assert!(!rendered.contains("s3cret"), "{rendered}");
}
