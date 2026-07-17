//! a thin, reusable SMTP sender ([`lettre`]-backed), configured explicitly
//! from host/port/from/credentials rather than from the environment. Reading
//! env vars into these parameters is glue code for the consuming binary's
//! own `main.rs`, not this crate.

use lettre::{
    AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    message::{Mailbox, MultiPart},
    transport::smtp::{authentication::Credentials, client::Tls},
};
use thiserror::Error;

/// errors from building or using a [`Mailer`]
#[derive(Debug, Error)]
pub enum MailerError {
    /// the parameters passed to [`Mailer::new`] don't add up to a usable
    /// configuration
    #[error("invalid SMTP configuration: {0}")]
    Config(String),
    /// building the underlying message/transport failed
    #[error(transparent)]
    Build(#[from] lettre::error::Error),
    /// a `from`/recipient address didn't parse as a [`Mailbox`]
    #[error(transparent)]
    Address(#[from] lettre::address::AddressError),
    /// the SMTP transport failed to send the message
    #[error(transparent)]
    Transport(#[from] lettre::transport::smtp::Error),
}

/// a configured SMTP sender: one transport plus a fixed `From` address.
#[derive(Debug)]
pub struct Mailer {
    transport: AsyncSmtpTransport<Tokio1Executor>,
    from: Mailbox,
}

impl Mailer {
    /// `credentials`: `Some((user, password))` for an authenticated relay,
    /// `None` for an unauthenticated local dev catcher (e.g. Mailpit/MailHog);
    /// see the TLS-strategy comment below for why that distinction matters.
    /// Taking the pair as one `Option` makes a half-configured relay (user
    /// without password or vice versa) unrepresentable, instead of silently
    /// falling back to the unauthenticated, TLS-less dev path.
    pub fn new(
        host: impl AsRef<str>,
        port: u16,
        from: impl AsRef<str>,
        credentials: Option<(String, String)>,
    ) -> Result<Self, MailerError> {
        let host = host.as_ref();
        let from = from.as_ref().parse::<Mailbox>()?;

        let credentials = credentials.map(|(user, password)| Credentials::new(user, password));

        // authenticated relays use TLS: implicit TLS on the SMTPS port (465),
        // STARTTLS everywhere else (587 submission, or any other port) - mixing
        // these up makes the client speak TLS at a plaintext-greeting server
        // (or vice versa), which rustls reports as a corrupt/InvalidContentType
        // message rather than a clear handshake error. An unauthenticated host
        // is assumed to be a local dev catcher (e.g. Mailpit/MailHog) with no TLS.
        let builder = match &credentials {
            Some(_) if port == 465 => AsyncSmtpTransport::<Tokio1Executor>::relay(host)?.port(port),
            Some(_) => AsyncSmtpTransport::<Tokio1Executor>::starttls_relay(host)?.port(port),
            None => AsyncSmtpTransport::<Tokio1Executor>::builder_dangerous(host)
                .port(port)
                .tls(Tls::None),
        };

        let builder = match credentials {
            Some(credentials) => builder.credentials(credentials),
            None => builder,
        };

        Ok(Self {
            transport: builder.build(),
            from,
        })
    }

    /// sends `subject`/`plain_body`/`html_body` to every address in `to` as
    /// a single email (one `To` header with all of them) - fine when every
    /// recipient may see the others, e.g. an internal staff list; a caller
    /// with external recipients who shouldn't see each other should call
    /// this once per recipient instead.
    pub async fn send(
        &self,
        to: &[String],
        subject: &str,
        plain_body: String,
        html_body: String,
    ) -> Result<(), MailerError> {
        let mut builder = Message::builder().from(self.from.clone());
        for recipient in to {
            builder = builder.to(recipient.parse::<Mailbox>()?);
        }

        let message = builder
            .subject(subject)
            .multipart(MultiPart::alternative_plain_html(plain_body, html_body))?;

        self.transport.send(message).await?;
        Ok(())
    }
}
