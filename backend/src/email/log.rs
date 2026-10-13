//! Development-only email "provider": writes the OTP (and the text body of
//! notification emails) to the server log instead of sending mail. `Config::validate` refuses `EMAIL_PROVIDER=log` when
//! `APP_ENV=production`, so this can never leak codes from a real deployment.

use secrecy::ExposeSecret;

use super::{BoxFuture, EmailSender, OtpEmail, OutboundEmail};

pub struct LogEmailSender;

impl LogEmailSender {
    pub fn new() -> Self {
        tracing::warn!(
            "EMAIL_PROVIDER=log: one-time codes will be written to the log (development only)"
        );
        Self
    }
}

impl Default for LogEmailSender {
    fn default() -> Self {
        Self::new()
    }
}

impl EmailSender for LogEmailSender {
    fn send_otp<'a>(&'a self, email: &'a OtpEmail) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            tracing::warn!(
                target: "zhimart_backend::email::dev_otp",
                "\n==================== DEV ONLY ====================\n\
                 DEV ONLY — OTP for {to}: {code}  (expires in {mins} min{ip})\n\
                 ==================================================",
                to = email.to,
                code = email.code.expose_secret(),
                mins = email.expires_in_minutes,
                ip = email.request_ip.map(|ip| format!(", requested from {ip}")).unwrap_or_default(),
            );
            Ok(())
        })
    }

    fn send_email<'a>(&'a self, email: &'a OutboundEmail) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(async move {
            anyhow::ensure!(!email.to.is_empty(), "no recipients");
            tracing::warn!(
                target: "zhimart_backend::email::dev_mail",
                "\n==================== DEV ONLY: EMAIL NOT SENT ====================\n\
                 To: {to}\nSubject: {subject}\n\n{text}\n\
                 =================================================================",
                to = email.to.join(", "),
                subject = email.subject,
                text = email.text,
            );
            Ok(())
        })
    }
}
