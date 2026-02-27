//! Email channel adapter (IMAP + SMTP).
//!
//! Polls IMAP for new emails and sends responses via SMTP.
//! Uses the subject line for agent routing (e.g., "\[coder\] Fix this bug").

use crate::types::{ChannelAdapter, ChannelContent, ChannelMessage, ChannelType, ChannelUser};
use async_std::net::TcpStream;
use async_trait::async_trait;
use chrono::Utc;
use futures::{Stream, TryStreamExt};
use lettre::message::header::ContentType;
use lettre::transport::smtp::authentication::Credentials;
use lettre::{Message, SmtpTransport, Transport};
use std::collections::HashSet;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{mpsc, watch};
use tokio_stream::wrappers::UnboundedReceiverStream;
use tracing::{debug, error, info, warn};
use zeroize::Zeroizing;

/// Email channel adapter using IMAP for receiving and SMTP for sending.
pub struct EmailAdapter {
    /// IMAP server host.
    imap_host: String,
    /// IMAP port (993 for TLS).
    imap_port: u16,
    /// SMTP server host.
    smtp_host: String,
    /// SMTP port (587 for STARTTLS).
    smtp_port: u16,
    /// Email address (used for both IMAP and SMTP).
    username: String,
    /// SECURITY: Password is zeroized on drop.
    password: Zeroizing<String>,
    /// How often to check for new emails.
    poll_interval: Duration,
    /// Which IMAP folders to monitor.
    folders: Vec<String>,
    /// Only process emails from these senders (empty = all).
    allowed_senders: Vec<String>,
    /// Shutdown signal.
    shutdown_tx: Arc<watch::Sender<bool>>,
    shutdown_rx: watch::Receiver<bool>,
}

impl EmailAdapter {
    /// Create a new email adapter.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        imap_host: String,
        imap_port: u16,
        smtp_host: String,
        smtp_port: u16,
        username: String,
        password: String,
        poll_interval_secs: u64,
        folders: Vec<String>,
        allowed_senders: Vec<String>,
    ) -> Self {
        let (shutdown_tx, shutdown_rx) = watch::channel(false);
        Self {
            imap_host,
            imap_port,
            smtp_host,
            smtp_port,
            username,
            password: Zeroizing::new(password),
            poll_interval: Duration::from_secs(poll_interval_secs),
            folders: if folders.is_empty() {
                vec!["INBOX".to_string()]
            } else {
                folders
            },
            allowed_senders,
            shutdown_tx: Arc::new(shutdown_tx),
            shutdown_rx,
        }
    }

    #[allow(dead_code)]
    fn is_allowed_sender(&self, sender: &str) -> bool {
        self.allowed_senders.is_empty() || self.allowed_senders.iter().any(|s| sender.contains(s))
    }

    /// Extract agent name from subject line brackets, e.g., "[coder] Fix the bug" -> Some("coder")
    #[allow(dead_code)]
    fn extract_agent_from_subject(subject: &str) -> Option<String> {
        let subject = subject.trim();
        if subject.starts_with('[') {
            if let Some(end) = subject.find(']') {
                let agent = &subject[1..end];
                if !agent.is_empty() {
                    return Some(agent.to_string());
                }
            }
        }
        None
    }

    /// Strip the agent tag from a subject line.
    #[allow(dead_code)]
    fn strip_agent_tag(subject: &str) -> String {
        let subject = subject.trim();
        if subject.starts_with('[') {
            if let Some(end) = subject.find(']') {
                return subject[end + 1..].trim().to_string();
            }
        }
        subject.to_string()
    }

    /// Parse raw RFC822 email into ChannelMessage.
    fn parse_email_message(
        body: &[u8],
        allowed_senders: &[String],
    ) -> Result<Option<ChannelMessage>, Box<dyn std::error::Error + Send + Sync>> {
        use std::collections::HashMap;
        use std::str;

        let body_str = str::from_utf8(body)?;

        // Simple header parsing (in production, use mail-parser crate)
        let mut from = String::new();
        let mut to = String::new();
        let mut subject = String::new();
        let mut message_id = None;
        let mut in_reply_to = None;
        let mut body_text = String::new();
        let mut in_body = false;

        for line in body_str.lines() {
            if line.is_empty() && !in_body {
                in_body = true;
                continue;
            }

            if in_body {
                body_text.push_str(line);
                body_text.push('\n');
            } else if let Some(addr) = line.strip_prefix("From: ") {
                from = Self::extract_email_address(addr);
            } else if let Some(addr) = line.strip_prefix("To: ") {
                to = Self::extract_email_address(addr);
            } else if let Some(subj) = line.strip_prefix("Subject: ") {
                subject = subj.to_string();
            } else if let Some(msg_id) = line.strip_prefix("Message-ID: ") {
                message_id = Some(msg_id.trim().to_string());
            } else if let Some(reply_to) = line.strip_prefix("In-Reply-To: ") {
                in_reply_to = Some(reply_to.trim().to_string());
            }
        }

        // Filter by allowed senders
        if !allowed_senders.is_empty() && !allowed_senders.iter().any(|s| from.contains(s)) {
            return Ok(None);
        }

        // Extract target agent name from subject (e.g., [agent-name] message)
        let target_agent_name = Self::extract_agent_from_subject(&subject);
        let clean_subject = Self::strip_agent_tag(&subject);

        let mut metadata = HashMap::new();
        metadata.insert(
            "subject".to_string(),
            serde_json::Value::String(clean_subject),
        );
        if !to.is_empty() {
            metadata.insert(
                "recipient_email".to_string(),
                serde_json::Value::String(to.clone()),
            );
        }
        if let Some(ref agent_name) = target_agent_name {
            metadata.insert(
                "target_agent_name".to_string(),
                serde_json::Value::String(agent_name.clone()),
            );
        }
        if let Some(ref reply_to) = in_reply_to {
            metadata.insert(
                "in_reply_to".to_string(),
                serde_json::Value::String(reply_to.clone()),
            );
        }

        Ok(Some(ChannelMessage {
            channel: ChannelType::Email,
            platform_message_id: message_id.unwrap_or_else(|| uuid::Uuid::new_v4().to_string()),
            sender: ChannelUser {
                platform_id: from.clone(),
                display_name: from,
                openfang_user: None,
            },
            content: ChannelContent::Text(body_text.trim().to_string()),
            target_agent: None, // Agent name is in metadata["target_agent_name"], resolved by bridge
            timestamp: Utc::now(),
            is_group: false,
            thread_id: in_reply_to,
            metadata,
        }))
    }

    /// Extract email address from "Name <email@example.com>" format.
    fn extract_email_address(from_field: &str) -> String {
        if let Some(start) = from_field.find('<') {
            if let Some(end) = from_field.find('>') {
                return from_field[start + 1..end].to_string();
            }
        }
        from_field.trim().to_string()
    }
}

#[async_trait]
impl ChannelAdapter for EmailAdapter {
    fn name(&self) -> &str {
        "email"
    }

    fn channel_type(&self) -> ChannelType {
        ChannelType::Email
    }

    async fn start(
        &self,
    ) -> Result<Pin<Box<dyn Stream<Item = ChannelMessage> + Send>>, Box<dyn std::error::Error>>
    {
        let (tx, rx) = mpsc::unbounded_channel::<ChannelMessage>();
        let poll_interval = self.poll_interval;
        let allowed_senders = self.allowed_senders.clone();
        let imap_host = self.imap_host.clone();
        let imap_port = self.imap_port;
        let username = self.username.clone();
        let password = self.password.clone();
        let folders = self.folders.clone();
        let mut shutdown_rx = self.shutdown_rx.clone();

        info!(
            "Starting email adapter (IMAP: {}:{}, polling every {:?})",
            imap_host, imap_port, poll_interval
        );

        tokio::spawn(async move {
            let mut seen_uids: HashSet<u32> = HashSet::new();

            loop {
                tokio::select! {
                    _ = shutdown_rx.changed() => {
                        info!("Email adapter shutting down");
                        break;
                    }
                    _ = tokio::time::sleep(poll_interval) => {}
                }

                debug!("Email poll cycle (IMAP {}:{})", imap_host, imap_port);

                // Connect to IMAP server
                let tcp_stream = match TcpStream::connect((imap_host.as_str(), imap_port)).await {
                    Ok(stream) => stream,
                    Err(e) => {
                        error!("Failed to connect to IMAP server: {}", e);
                        continue;
                    }
                };

                let tls = async_native_tls::TlsConnector::new();
                let tls_stream = match tls.connect(&imap_host, tcp_stream).await {
                    Ok(stream) => stream,
                    Err(e) => {
                        error!("Failed to establish TLS connection: {}", e);
                        continue;
                    }
                };

                let client = async_imap::Client::new(tls_stream);
                let mut imap_session = match client.login(&username, password.as_str()).await {
                    Ok(session) => session,
                    Err((e, _)) => {
                        error!("IMAP login failed: {}", e);
                        continue;
                    }
                };

                // Process each folder
                for folder in &folders {
                    if let Err(e) = imap_session.select(folder).await {
                        warn!("Failed to select folder {}: {}", folder, e);
                        continue;
                    }

                    // Search for all messages (we'll track seen ones ourselves)
                    let uids: Vec<u32> = match imap_session.uid_search("ALL").await {
                        Ok(uids) => uids.into_iter().collect(),
                        Err(e) => {
                            warn!("IMAP search failed in {}: {}", folder, e);
                            continue;
                        }
                    };

                    // Fetch new messages (not in seen_uids)
                    for uid in uids.iter() {
                        if seen_uids.contains(uid) {
                            continue;
                        }

                        let messages: Vec<_> =
                            match imap_session.uid_fetch(uid.to_string(), "RFC822").await {
                                Ok(msgs) => msgs.try_collect().await.unwrap_or_default(),
                                Err(e) => {
                                    warn!("Failed to fetch message {}: {}", uid, e);
                                    continue;
                                }
                            };

                        for msg in messages.iter() {
                            if let Some(body) = msg.body() {
                                match Self::parse_email_message(body, &allowed_senders) {
                                    Ok(Some(channel_msg)) => {
                                        debug!(
                                            "Parsed email from {}",
                                            channel_msg.sender.platform_id
                                        );
                                        if tx.send(channel_msg).is_err() {
                                            warn!("Channel closed, stopping email polling");
                                            let _ = imap_session.logout().await;
                                            return;
                                        }
                                        seen_uids.insert(*uid);
                                    }
                                    Ok(None) => {
                                        debug!("Email filtered (sender not allowed)");
                                        seen_uids.insert(*uid);
                                    }
                                    Err(e) => {
                                        error!("Failed to parse email: {}", e);
                                    }
                                }
                            }
                        }
                    }
                }

                // Logout
                if let Err(e) = imap_session.logout().await {
                    warn!("IMAP logout failed: {}", e);
                }
            }
        });

        Ok(Box::pin(UnboundedReceiverStream::new(rx)))
    }

    async fn send(
        &self,
        user: &ChannelUser,
        content: ChannelContent,
    ) -> Result<(), Box<dyn std::error::Error>> {
        let text = match content {
            ChannelContent::Text(t) => t,
            _ => {
                warn!("Unsupported email content type for {}", user.platform_id);
                return Ok(());
            }
        };

        // Build email message
        let subject = format!("Re: {}", text.lines().next().unwrap_or("Response"));
        let email = Message::builder()
            .from(self.username.parse()?)
            .to(user.platform_id.parse()?)
            .subject(subject)
            .header(ContentType::TEXT_PLAIN)
            .body(text.clone())?;

        // Send via SMTP
        let creds = Credentials::new(self.username.clone(), self.password.to_string());

        let mailer = SmtpTransport::starttls_relay(&self.smtp_host)?
            .port(self.smtp_port)
            .credentials(creds)
            .build();

        match mailer.send(&email) {
            Ok(_) => {
                info!(
                    "✅ Sent email to {} ({} chars)",
                    user.platform_id,
                    text.len()
                );
                debug!(
                    "SMTP: {}:{} -> {}",
                    self.smtp_host, self.smtp_port, user.platform_id
                );
                Ok(())
            }
            Err(e) => {
                error!("Failed to send email to {}: {}", user.platform_id, e);
                Err(Box::new(e))
            }
        }
    }

    async fn stop(&self) -> Result<(), Box<dyn std::error::Error>> {
        let _ = self.shutdown_tx.send(true);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_email_adapter_creation() {
        let adapter = EmailAdapter::new(
            "imap.gmail.com".to_string(),
            993,
            "smtp.gmail.com".to_string(),
            587,
            "user@gmail.com".to_string(),
            "password".to_string(),
            30,
            vec![],
            vec![],
        );
        assert_eq!(adapter.name(), "email");
        assert_eq!(adapter.folders, vec!["INBOX".to_string()]);
    }

    #[test]
    fn test_allowed_senders() {
        let adapter = EmailAdapter::new(
            "imap.example.com".to_string(),
            993,
            "smtp.example.com".to_string(),
            587,
            "bot@example.com".to_string(),
            "pass".to_string(),
            30,
            vec![],
            vec!["boss@company.com".to_string()],
        );
        assert!(adapter.is_allowed_sender("boss@company.com"));
        assert!(!adapter.is_allowed_sender("random@other.com"));

        let open = EmailAdapter::new(
            "imap.example.com".to_string(),
            993,
            "smtp.example.com".to_string(),
            587,
            "bot@example.com".to_string(),
            "pass".to_string(),
            30,
            vec![],
            vec![],
        );
        assert!(open.is_allowed_sender("anyone@anywhere.com"));
    }

    #[test]
    fn test_extract_agent_from_subject() {
        assert_eq!(
            EmailAdapter::extract_agent_from_subject("[coder] Fix the bug"),
            Some("coder".to_string())
        );
        assert_eq!(
            EmailAdapter::extract_agent_from_subject("[researcher] Find papers on AI"),
            Some("researcher".to_string())
        );
        assert_eq!(
            EmailAdapter::extract_agent_from_subject("No brackets here"),
            None
        );
        assert_eq!(
            EmailAdapter::extract_agent_from_subject("[] Empty brackets"),
            None
        );
    }

    #[test]
    fn test_strip_agent_tag() {
        assert_eq!(
            EmailAdapter::strip_agent_tag("[coder] Fix the bug"),
            "Fix the bug"
        );
        assert_eq!(EmailAdapter::strip_agent_tag("No brackets"), "No brackets");
    }

    #[test]
    fn test_extract_email_address() {
        // Standard format: Name <email@domain.com>
        assert_eq!(
            EmailAdapter::extract_email_address("John Doe <john@example.com>"),
            "john@example.com"
        );
        // Just email
        assert_eq!(
            EmailAdapter::extract_email_address("jane@example.com"),
            "jane@example.com"
        );
        // With extra whitespace
        assert_eq!(
            EmailAdapter::extract_email_address("  test@test.com  "),
            "test@test.com"
        );
    }

    #[test]
    fn test_parse_email_message() {
        let raw_email = b"From: sender@example.com\r
Subject: [test-agent] Hello\r
Message-ID: <abc123@mail.example.com>\r
\r
This is the body of the email.
It has multiple lines.
";

        let result = EmailAdapter::parse_email_message(raw_email, &[]).unwrap();
        assert!(result.is_some());

        let msg = result.unwrap();
        assert_eq!(msg.sender.platform_id, "sender@example.com");
        assert_eq!(msg.channel, ChannelType::Email);
        assert!(matches!(msg.content, ChannelContent::Text(_)));

        if let ChannelContent::Text(text) = msg.content {
            assert!(text.contains("body of the email"));
        }

        // Check metadata
        assert!(msg.metadata.contains_key("subject"));
    }

    #[test]
    fn test_parse_email_with_agent_routing() {
        let raw_email = b"From: user@example.com\r
Subject: [my-agent] Please help\r
\r
I need assistance.
";

        let result = EmailAdapter::parse_email_message(raw_email, &[]).unwrap();
        assert!(result.is_some());

        let msg = result.unwrap();
        // Agent routing is extracted and stored in metadata
        assert!(msg.metadata.contains_key("target_agent_name"));
        let agent_name = msg
            .metadata
            .get("target_agent_name")
            .unwrap()
            .as_str()
            .unwrap();
        assert_eq!(agent_name, "my-agent");

        // target_agent field is None (will be resolved by bridge)
        assert!(msg.target_agent.is_none());

        // Subject should be cleaned
        let subject = msg.metadata.get("subject").unwrap().as_str().unwrap();
        assert_eq!(subject, "Please help");
    }

    #[test]
    fn test_parse_email_filters_senders() {
        let raw_email = b"From: blocked@spam.com\r
Subject: Test\r
\r
Body
";

        // Should be filtered out
        let result =
            EmailAdapter::parse_email_message(raw_email, &["allowed@example.com".to_string()])
                .unwrap();
        assert!(result.is_none());

        // Should pass through
        let raw_allowed = b"From: allowed@example.com\r
Subject: Test\r
\r
Body
";
        let result =
            EmailAdapter::parse_email_message(raw_allowed, &["allowed@example.com".to_string()])
                .unwrap();
        assert!(result.is_some());
    }

    #[test]
    fn test_parse_email_with_thread() {
        let raw_email = b"From: user@example.com\r
Subject: Re: Previous conversation\r
Message-ID: <msg2@mail.com>\r
In-Reply-To: <msg1@mail.com>\r
\r
Replying to your message.
";

        let result = EmailAdapter::parse_email_message(raw_email, &[]).unwrap();
        assert!(result.is_some());

        let msg = result.unwrap();
        assert!(msg.thread_id.is_some());
        assert_eq!(msg.thread_id.unwrap(), "<msg1@mail.com>");

        // Check metadata has in_reply_to
        assert!(msg.metadata.contains_key("in_reply_to"));
    }

    #[test]
    fn test_adapter_defaults() {
        let adapter = EmailAdapter::new(
            "imap.test.com".to_string(),
            993,
            "smtp.test.com".to_string(),
            587,
            "bot@test.com".to_string(),
            "password".to_string(),
            60,
            vec![], // Empty folders should default to INBOX
            vec![],
        );

        assert_eq!(adapter.folders, vec!["INBOX".to_string()]);
        assert_eq!(adapter.poll_interval, Duration::from_secs(60));
    }
}
