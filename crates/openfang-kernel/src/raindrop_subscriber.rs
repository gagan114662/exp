//! Raindrop incident subscriber for Telegram notification forwarding.

use openfang_types::config::RaindropConfig;
use openfang_types::raindrop::{RaindropIncident, RaindropSeverity};
use reqwest::Client;
use tokio::time::{sleep, Duration};
use tracing::{debug, error, info, warn};

/// Raindrop incident subscriber.
pub struct RaindropSubscriber {
    config: RaindropConfig,
    telegram_bot_token: Option<String>,
    client: Client,
}

impl RaindropSubscriber {
    /// Create a new Raindrop subscriber.
    pub fn new(config: RaindropConfig, telegram_bot_token: Option<String>) -> Self {
        Self {
            config,
            telegram_bot_token,
            client: Client::new(),
        }
    }

    /// Subscribe to incident stream and forward to Telegram.
    pub async fn subscribe_and_forward(&self) -> Result<(), String> {
        if !self.config.enabled {
            return Err("Raindrop integration disabled".to_string());
        }

        info!("Starting Raindrop incident subscription");

        loop {
            match self.try_subscribe().await {
                Ok(_) => {
                    warn!("Raindrop SSE stream ended, reconnecting...");
                }
                Err(e) => {
                    warn!("Raindrop subscription failed: {}, retrying in 30s", e);
                    sleep(Duration::from_secs(30)).await;
                }
            }
        }
    }

    async fn try_subscribe(&self) -> Result<(), String> {
        let url = format!(
            "{}/v1/incidents/stream",
            self.config.api_url.trim_end_matches('/')
        );

        let mut request = self.client.get(&url);
        if let Some(ref token) = self.config.api_token {
            request = request.header("Authorization", format!("Bearer {}", token));
        }
        let response = request
            .send()
            .await
            .map_err(|e| format!("Failed to connect to Raindrop: {}", e))?;

        if !response.status().is_success() {
            return Err(format!("Raindrop returned status: {}", response.status()));
        }

        // Parse SSE stream
        let mut stream = response.bytes_stream();

        use futures::StreamExt;
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(|e| format!("Stream error: {}", e))?;
            let text = String::from_utf8_lossy(&chunk);

            // Parse SSE format: "data: {...}\n\n"
            for line in text.lines() {
                if let Some(json_str) = line.strip_prefix("data: ") {
                    if let Ok(incident) = serde_json::from_str::<RaindropIncident>(json_str) {
                        if let Err(e) = self.forward_incident(incident).await {
                            warn!("Failed to forward incident: {}", e);
                        }
                    }
                }
            }
        }

        Ok(())
    }

    async fn forward_incident(&self, incident: RaindropIncident) -> Result<(), String> {
        // Look up chat_id for workspace
        let chat_id = self
            .config
            .workspace_chat_mapping
            .get(&incident.workspace_id)
            .ok_or_else(|| {
                format!(
                    "No chat_id configured for workspace {}",
                    incident.workspace_id
                )
            })?;

        // Format incident message
        let severity_emoji = match incident.severity {
            RaindropSeverity::Critical => "🔴",
            RaindropSeverity::High => "🟠",
            RaindropSeverity::Medium => "🟡",
            RaindropSeverity::Low => "🟢",
        };

        let text = format!(
            "{} [incident:{}]\n\
            Workspace: {}\n\
            Agent: {}\n\
            Source: {}\n\
            Label: {}\n\
            Severity: {:?}\n\
            Message: {}",
            severity_emoji,
            incident.id,
            incident.workspace_id,
            incident.agent_id,
            incident.source_system.as_deref().unwrap_or("unknown"),
            incident.signal_label,
            incident.severity,
            incident.latest_message
        );

        // Send message via Telegram Bot API directly
        let bot_token = self
            .telegram_bot_token
            .as_ref()
            .ok_or_else(|| "Telegram bot token not configured".to_string())?;

        let url = format!("https://api.telegram.org/bot{}/sendMessage", bot_token);
        let payload = serde_json::json!({
            "chat_id": chat_id,
            "text": text,
            "parse_mode": "HTML"
        });

        debug!("Sending Telegram message to chat_id {}", chat_id);

        let response = self
            .client
            .post(&url)
            .json(&payload)
            .send()
            .await
            .map_err(|e| format!("Failed to send Telegram request: {}", e))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!("Telegram API error: {} - {}", status, body);
            return Err(format!("Telegram API returned {}: {}", status, body));
        }

        info!("✅ Forwarded incident {} to Telegram", incident.id);
        Ok(())
    }
}
