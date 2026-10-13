//! Outbound email. `EmailSender` is object-safe so the provider (SES in
//! production, log-only in development) is chosen at startup.

pub mod log;
pub mod ses;
pub mod templates;

use std::{future::Future, net::IpAddr, pin::Pin, sync::Arc};

use secrecy::SecretString;

use crate::config::{Config, EmailProvider};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// A one-time login code to deliver. `code` is secret: never log it outside
/// the development `log` provider.
pub struct OtpEmail {
    pub to: String,
    pub display_name: String,
    pub code: SecretString,
    pub expires_in_minutes: u64,
    pub request_ip: Option<IpAddr>,
}

/// A fully rendered transactional email (e.g. the admin "new order" notification).
pub struct OutboundEmail {
    pub to: Vec<String>,
    pub subject: String,
    pub html: String,
    pub text: String,
}

pub trait EmailSender: Send + Sync + 'static {
    fn send_otp<'a>(&'a self, email: &'a OtpEmail) -> BoxFuture<'a, anyhow::Result<()>>;

    /// Send a rendered email. Providers that cannot (test doubles) keep the default.
    fn send_email<'a>(&'a self, _email: &'a OutboundEmail) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async { anyhow::bail!("this email provider does not support send_email") })
    }
}

/// Build the configured email provider.
pub async fn build_sender(config: &Config) -> anyhow::Result<Arc<dyn EmailSender>> {
    Ok(match config.email_provider {
        EmailProvider::Ses => Arc::new(ses::SesEmailSender::from_env(config).await?),
        EmailProvider::Log => Arc::new(log::LogEmailSender::new()),
    })
}
