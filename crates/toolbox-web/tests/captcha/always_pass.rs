use toolbox_web::captcha::{AlwaysPass, CaptchaVerifier};

#[tokio::test]
async fn the_development_verifier_accepts_everything() {
    assert!(AlwaysPass.verify("anything", None).await.unwrap());
    assert!(AlwaysPass.verify("", Some("203.0.113.9")).await.unwrap());
}
