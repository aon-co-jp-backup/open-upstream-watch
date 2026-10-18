//! Alert e-mails. The SMTP settings use the same environment variable names as open-easy-web
//! (`OPEN_EASYWEB_SMTP_HOST/PORT/USERNAME/PASSWORD/FROM`), so the one existing mail account is reused and no new
//! secret has to be created. Secrets are read from the environment only and are never logged.

use anyhow::{Context, Result};
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use std::sync::{Arc, Mutex};

/// Anything that can deliver an alert. The pipeline only depends on this trait (tests use [`Recorder`]).
#[allow(async_fn_in_trait)]
pub trait Notifier: Send + Sync {
    async fn notify(&self, subject: &str, body: &str) -> Result<()>;
}

#[derive(Debug, Clone)]
pub struct SmtpConfig {
    pub host: String,
    pub port: u16,
    pub username: String,
    pub password: String,
    pub from: String,
}

impl SmtpConfig {
    pub fn from_env() -> Option<Self> {
        Some(Self {
            host: std::env::var("OPEN_EASYWEB_SMTP_HOST").ok()?,
            port: std::env::var("OPEN_EASYWEB_SMTP_PORT").ok().and_then(|v| v.parse().ok()).unwrap_or(587),
            username: std::env::var("OPEN_EASYWEB_SMTP_USERNAME").ok()?,
            password: std::env::var("OPEN_EASYWEB_SMTP_PASSWORD").ok()?,
            from: std::env::var("OPEN_EASYWEB_SMTP_FROM").ok()?,
        })
    }
}

pub struct SmtpNotifier {
    pub smtp: SmtpConfig,
    pub to: Vec<String>,
}

fn send_blocking(smtp: &SmtpConfig, to: &str, subject: &str, body: &str) -> Result<()> {
    let email = Message::builder()
        .from(smtp.from.parse().context("invalid From address")?)
        .to(to.parse().context("invalid To address")?)
        .subject(subject)
        .header(ContentType::TEXT_PLAIN)
        .body(body.to_string())?;
    let mailer = SmtpTransport::starttls_relay(&smtp.host)?
        .port(smtp.port)
        .credentials(Credentials::new(smtp.username.clone(), smtp.password.clone()))
        .build();
    mailer.send(&email).context("SMTP send failed")?;
    Ok(())
}

impl Notifier for SmtpNotifier {
    async fn notify(&self, subject: &str, body: &str) -> Result<()> {
        let mut last = None;
        for to in &self.to {
            let (smtp, to, subject, body) = (self.smtp.clone(), to.clone(), subject.to_string(), body.to_string());
            // lettre's SMTP client is synchronous: keep it off the async runtime threads
            let r = tokio::task::spawn_blocking(move || send_blocking(&smtp, &to, &subject, &body)).await?;
            if let Err(e) = r {
                last = Some(e);
            }
        }
        match last {
            Some(e) => Err(e),
            None => Ok(()),
        }
    }
}

/// Used when no recipient or SMTP configuration exists: alerts are only logged.
pub struct LogOnly;

impl Notifier for LogOnly {
    async fn notify(&self, subject: &str, _body: &str) -> Result<()> {
        tracing::warn!("alert (no mail configured): {subject}");
        Ok(())
    }
}

/// Collects alerts in memory (tests).
#[derive(Default, Clone)]
pub struct Recorder {
    pub sent: Arc<Mutex<Vec<(String, String)>>>,
}

impl Notifier for Recorder {
    async fn notify(&self, subject: &str, body: &str) -> Result<()> {
        self.sent.lock().unwrap().push((subject.to_string(), body.to_string()));
        Ok(())
    }
}

/// The notifier chosen from the configuration: SMTP when recipients and the shared SMTP settings exist, otherwise log only.
pub enum AnyNotifier {
    Smtp(SmtpNotifier),
    Log(LogOnly),
}

impl AnyNotifier {
    pub fn from_config(cfg: &crate::config::Config) -> Self {
        match (SmtpConfig::from_env(), cfg.alert.to.is_empty()) {
            (Some(smtp), false) => AnyNotifier::Smtp(SmtpNotifier { smtp, to: cfg.alert.to.clone() }),
            _ => AnyNotifier::Log(LogOnly),
        }
    }

    pub fn is_mail(&self) -> bool {
        matches!(self, AnyNotifier::Smtp(_))
    }
}

impl Notifier for AnyNotifier {
    async fn notify(&self, subject: &str, body: &str) -> Result<()> {
        match self {
            AnyNotifier::Smtp(n) => n.notify(subject, body).await,
            AnyNotifier::Log(n) => n.notify(subject, body).await,
        }
    }
}
