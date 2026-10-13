//! AWS SES v2 delivery. Credentials come from the standard AWS provider chain
//! (env vars, shared profile, SSO, ECS task role, EC2 instance profile, IRSA),
//! so no keys are ever part of this application's own configuration.

use std::time::Duration;

use anyhow::{Context, bail};
use aws_config::{BehaviorVersion, retry::RetryConfig, timeout::TimeoutConfig};
use aws_sdk_sesv2::{
    Client,
    error::DisplayErrorContext,
    types::{Body, Content, Destination, EmailContent, Message},
};

use super::{BoxFuture, EmailSender, OtpEmail, OutboundEmail, templates};
use crate::config::Config;

const CHARSET: &str = "UTF-8";
/// SES v2 limit on recipients per message.
const SES_MAX_RECIPIENTS: usize = 50;

/// SES latency budget. The whole send (all retries) must finish well inside the
/// HTTP request timeout so the login endpoint answers with a clean
/// `service_unavailable` instead of being cut off by the edge timeout.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);
const ATTEMPT_TIMEOUT: Duration = Duration::from_secs(4);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(9);
const MAX_ATTEMPTS: u32 = 3;

pub struct SesEmailSender {
    client: Client,
    from: String,
    configuration_set: Option<String>,
}

impl SesEmailSender {
    pub async fn from_env(config: &Config) -> anyhow::Result<Self> {
        let sdk_config = aws_config::defaults(BehaviorVersion::latest())
            .timeout_config(
                TimeoutConfig::builder()
                    .connect_timeout(CONNECT_TIMEOUT)
                    .operation_attempt_timeout(ATTEMPT_TIMEOUT)
                    .operation_timeout(OPERATION_TIMEOUT)
                    .build(),
            )
            .retry_config(RetryConfig::standard().with_max_attempts(MAX_ATTEMPTS))
            .load()
            .await;

        let Some(region) = sdk_config.region() else {
            bail!("EMAIL_PROVIDER=ses requires an AWS region (set AWS_REGION)");
        };
        if sdk_config.credentials_provider().is_none() {
            bail!("EMAIL_PROVIDER=ses but no AWS credentials provider could be configured");
        }
        tracing::info!(
            region = %region,
            from = %config.ses_from_address,
            configuration_set = config.ses_configuration_set.as_deref().unwrap_or("-"),
            "AWS SES email sender configured"
        );

        Ok(Self {
            client: Client::new(&sdk_config),
            from: config.ses_from_address.clone(),
            configuration_set: config.ses_configuration_set.clone(),
        })
    }

    /// One SES `SendEmail` call (simple content, UTF-8). Returns the MessageId.
    /// Never logs or returns message content.
    async fn send_simple(
        &self,
        to: &[String],
        subject: &str,
        html: &str,
        text: &str,
    ) -> anyhow::Result<String> {
        let content = |data: &str| Content::builder().data(data).charset(CHARSET).build();
        let message = Message::builder()
            .subject(content(subject).context("building SES subject")?)
            .body(
                Body::builder()
                    .html(content(html).context("building SES html body")?)
                    .text(content(text).context("building SES text body")?)
                    .build(),
            )
            .build();

        let output = self
            .client
            .send_email()
            .from_email_address(&self.from)
            .destination(
                Destination::builder()
                    .set_to_addresses(Some(to.to_vec()))
                    .build(),
            )
            .content(EmailContent::builder().simple(message).build())
            .set_configuration_set_name(self.configuration_set.clone())
            .send()
            .await
            // DisplayErrorContext renders the SES error code/message chain; it
            // never includes the request body (and thus never the content).
            .map_err(|e| anyhow::anyhow!("SES SendEmail failed: {}", DisplayErrorContext(&e)))?;
        Ok(output.message_id().unwrap_or("-").to_owned())
    }

    async fn send_otp_mail(&self, email: &OtpEmail) -> anyhow::Result<()> {
        let rendered = templates::render_otp_email(email, chrono::Utc::now());
        let message_id = self
            .send_simple(
                std::slice::from_ref(&email.to),
                &rendered.subject,
                rendered.html.as_str(),
                rendered.text.as_str(),
            )
            .await?;
        tracing::info!(message_id, "OTP email accepted by SES");
        Ok(())
    }

    /// Sends `email.to` as one message (chunked at SES's 50-recipient limit).
    /// Admin notifications share one message; customer confirmations are
    /// always rendered with exactly one recipient, so customers never see
    /// each other. A failed chunk fails the whole send so the outbox retries
    /// it. The configuration set (if any) applies, so bounces/complaints are
    /// published for these messages too.
    async fn send_outbound(&self, email: &OutboundEmail) -> anyhow::Result<()> {
        anyhow::ensure!(!email.to.is_empty(), "no recipients");
        for chunk in email.to.chunks(SES_MAX_RECIPIENTS) {
            let message_id = self
                .send_simple(chunk, &email.subject, &email.html, &email.text)
                .await?;
            tracing::info!(
                message_id,
                recipients = chunk.len(),
                "notification email accepted by SES"
            );
        }
        Ok(())
    }
}

impl EmailSender for SesEmailSender {
    fn send_otp<'a>(&'a self, email: &'a OtpEmail) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(self.send_otp_mail(email))
    }

    fn send_email<'a>(&'a self, email: &'a OutboundEmail) -> BoxFuture<'a, anyhow::Result<()>> {
        Box::pin(self.send_outbound(email))
    }
}
