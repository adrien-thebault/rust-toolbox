//! tests `src/mail_tools.rs`: configuration validation. Actually delivering
//! mail needs an SMTP server, so `send` is only exercised up to the point
//! where it would touch the network (address parsing happens first).

use rust_toolbox::mail_tools::{Mailer, MailerError};

#[test]
fn rejects_an_invalid_from_address() {
    let err = Mailer::new("localhost", 2525, "not an address", None)
        .expect_err("a from that isn't a mailbox is a config error");
    assert!(matches!(err, MailerError::Address(_)));
}

#[test]
fn builds_for_authenticated_and_unauthenticated_configs() {
    // local dev catcher: no credentials
    assert!(Mailer::new("localhost", 2525, "from@example.com", None).is_ok());
    // authenticated relay, implicit TLS (465) and STARTTLS (anything else)
    assert!(
        Mailer::new(
            "smtp.example.com",
            465,
            "from@example.com",
            Some(("user".to_string(), "password".to_string())),
        )
        .is_ok()
    );
    assert!(
        Mailer::new(
            "smtp.example.com",
            587,
            "from@example.com",
            Some(("user".to_string(), "password".to_string())),
        )
        .is_ok()
    );
}

#[tokio::test]
async fn send_rejects_an_invalid_recipient_before_touching_the_network() {
    let mailer = Mailer::new("localhost", 2525, "from@example.com", None).expect("builds");

    let result = mailer
        .send(
            &["not an address".to_string()],
            "subject",
            "plain".to_string(),
            "<p>html</p>".to_string(),
        )
        .await;
    assert!(matches!(result, Err(MailerError::Address(_))));
}
