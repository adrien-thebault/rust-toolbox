use toolbox_web::captcha::ThirdPartyCaptcha;

/// A Debug of a config struct ends up in a log.
#[test]
fn debug_never_prints_the_secret() {
    let verifier = ThirdPartyCaptcha::new(
        "https://challenges.cloudflare.com/turnstile/v0/siteverify",
        "s3cret",
    )
    .unwrap();
    let rendered = format!("{verifier:?}");
    assert!(!rendered.contains("s3cret"), "{rendered}");
}

#[test]
fn debug_shows_the_endpoint() {
    let verifier = ThirdPartyCaptcha::new("https://example.test/siteverify", "s3cret").unwrap();
    let rendered = format!("{verifier:?}");
    assert!(rendered.contains("example.test"), "{rendered}");
}
