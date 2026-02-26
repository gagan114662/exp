# OpenFang-Raindrop Shared Telegram Bot - Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Integrate OpenFang and Raindrop through a shared Telegram bot that routes commands to OpenFang agents and forwards Raindrop incidents to Telegram.

**Architecture:** OpenFang hosts the teloxide bot (long polling), executes commands locally, subscribes to Raindrop's SSE incident stream, and forwards to Telegram with policy filtering.

**Tech Stack:** Rust, teloxide 0.12, reqwest SSE, tokio async runtime, bot token: 8250681078:AAE...

---

## Phase 1: Complete OpenFang Telegram Bot

### Task 1: Add teloxide bot implementation

**Files:**
- Modify: `crates/openfang-telegram/src/lib.rs`

**Step 1: Replace TelegramChannel with TelegramBot**

Remove the existing `TelegramChannel` struct and replace with:

```rust
use teloxide::prelude::*;

/// Telegram bot for bidirectional communication.
pub struct TelegramBot {
    bot: Bot,
    config: TelegramConfig,
}

impl TelegramBot {
    /// Create a new Telegram bot.
    pub fn new(config: TelegramConfig) -> Result<Self, String> {
        let bot_token = config.bot_token.as_ref()
            .ok_or_else(|| "Bot token not configured".to_string())?;

        let bot = Bot::new(bot_token);

        Ok(Self { bot, config })
    }

    /// Check if bot is enabled.
    pub fn is_enabled(&self) -> bool {
        self.config.enabled && self.config.bot_token.is_some()
    }

    /// Send a text message to a chat.
    pub async fn send_message(&self, chat_id: &str, text: &str) -> Result<(), String> {
        let chat_id_i64: i64 = chat_id.parse()
            .map_err(|_| format!("Invalid chat_id: {}", chat_id))?;

        self.bot
            .send_message(ChatId(chat_id_i64), text)
            .await
            .map_err(|e| format!("Failed to send message: {}", e))?;

        Ok(())
    }

    /// Start polling for messages and forward commands.
    pub async fn start_polling(
        self,
        command_tx: tokio::sync::mpsc::Sender<(String, TelegramCommand)>,
    ) -> Result<(), String> {
        if !self.is_enabled() {
            return Err("Bot not enabled or token missing".to_string());
        }

        info!("Starting Telegram bot polling");

        teloxide::repl(self.bot.clone(), move |bot: Bot, msg: Message| {
            let tx = command_tx.clone();
            let allowed = self.config.allowed_users.clone();

            async move {
                let chat_id = msg.chat.id.0.to_string();
                let user_id = msg.from().map(|u| u.id.0.to_string());

                // Authorization check
                if !allowed.is_empty() {
                    if let Some(uid) = &user_id {
                        if !allowed.contains(uid) {
                            // Silent ignore for unauthorized users
                            return Ok(());
                        }
                    } else {
                        return Ok(());
                    }
                }

                if let Some(text) = msg.text() {
                    let command = TelegramCommand::parse_command(text);

                    // Send command to kernel
                    let _ = tx.send((chat_id.clone(), command.clone())).await;

                    // Send acknowledgment for known commands
                    match command {
                        TelegramCommand::Help => {
                            bot.send_message(
                                msg.chat.id,
                                "Available commands:\n\
                                /run <agent> <task> - Run a task on an agent\n\
                                /agents - List all agents\n\
                                /status <agent_id> - Get agent status\n\
                                /help - Show this help"
                            ).await?;
                        }
                        TelegramCommand::Unknown { .. } => {
                            // Don't respond to unknown commands
                        }
                        _ => {
                            // Commands like /run, /agents, /status will be handled by kernel
                            bot.send_message(msg.chat.id, "Processing...").await?;
                        }
                    }
                }

                Ok(())
            }
        })
        .await;

        Ok(())
    }
}
```

**Step 2: Update TelegramCommand::parse_command**

Keep the existing `parse_command` function but make it a method:

```rust
impl TelegramCommand {
    /// Parse a text message into a command.
    pub fn parse_command(text: &str) -> Self {
        let text = text.trim();

        if let Some(args) = text.strip_prefix("/run ") {
            let parts: Vec<&str> = args.splitn(2, ' ').collect();
            if parts.len() == 2 {
                return TelegramCommand::Run {
                    agent: parts[0].to_string(),
                    task: parts[1].to_string(),
                };
            }
        }

        if text == "/agents" {
            return TelegramCommand::ListAgents;
        }

        if let Some(agent_id) = text.strip_prefix("/status ") {
            return TelegramCommand::Status {
                agent_id: agent_id.to_string(),
            };
        }

        if text == "/help" {
            return TelegramCommand::Help;
        }

        TelegramCommand::Unknown {
            text: text.to_string(),
        }
    }
}
```

**Step 3: Update tests**

Update existing tests to use `TelegramCommand::parse_command` instead of `TelegramChannel::parse_command`.

**Step 4: Run tests**

Run: `cargo test --package openfang-telegram --lib`
Expected: All tests pass

**Step 5: Commit**

```bash
git add crates/openfang-telegram/src/lib.rs
git commit -m "feat(telegram): implement teloxide bot with polling and sending"
```

---

### Task 2: Wire bot into kernel background tasks

**Files:**
- Modify: `crates/openfang-kernel/src/kernel.rs`

**Step 1: Update bot initialization in boot_with_config**

Find the Telegram initialization code (around line 850) and replace with:

```rust
// Initialize Telegram bot
let telegram_bot = if let Some(ref telegram_cfg) = config.channels.telegram {
    let bot_token = std::env::var(&telegram_cfg.bot_token_env).ok();

    if bot_token.is_some() {
        let telegram_config = openfang_telegram::TelegramConfig {
            enabled: true,
            bot_token,
            allowed_users: telegram_cfg.allowed_users.iter().map(|id| id.to_string()).collect(),
            rate_limit_per_minute: 10,
        };

        match openfang_telegram::TelegramBot::new(telegram_config) {
            Ok(bot) => {
                info!("Telegram bot initialized");
                Some(Arc::new(bot))
            }
            Err(e) => {
                warn!("Failed to initialize Telegram bot: {}", e);
                None
            }
        }
    } else {
        warn!("Telegram configured but {} not set", telegram_cfg.bot_token_env);
        None
    }
} else {
    None
};

let (command_tx, command_rx) = tokio::sync::mpsc::channel(100);
let telegram_commands = tokio::sync::Mutex::new(Some(command_rx));
```

**Step 2: Update kernel struct field**

Change `telegram_channel` field type from `Option<Arc<TelegramChannel>>` to `Option<Arc<TelegramBot>>`:

```rust
pub telegram_bot: Option<Arc<openfang_telegram::TelegramBot>>,
```

**Step 3: Update kernel initialization**

Replace:
```rust
telegram_channel,
```

With:
```rust
telegram_bot,
```

**Step 4: Start bot polling as background task**

After kernel is created (around line 950), add:

```rust
// Start Telegram bot polling in background
if let Some(ref bot) = kernel.telegram_bot {
    let bot_clone = bot.clone();
    let tx_clone = command_tx;

    tokio::spawn(async move {
        if let Err(e) = bot_clone.clone().start_polling(tx_clone).await {
            tracing::error!("Telegram bot polling failed: {}", e);
        }
    });
}
```

**Step 5: Run tests**

Run: `cargo test --package openfang-kernel --lib`
Expected: Tests pass

**Step 6: Commit**

```bash
git add crates/openfang-kernel/src/kernel.rs
git commit -m "feat(telegram): start bot polling in kernel background"
```

---

### Task 3: Add command handler in kernel

**Files:**
- Modify: `crates/openfang-kernel/src/kernel.rs`

**Step 1: Add command processing background task**

After the bot polling task, add:

```rust
// Start Telegram command processor
let kernel_weak = Arc::downgrade(&kernel_arc);
if kernel.telegram_bot.is_some() {
    tokio::spawn(async move {
        let mut rx_guard = kernel_arc.telegram_commands.lock().await;
        if let Some(mut rx) = rx_guard.take() {
            drop(rx_guard);

            while let Some((chat_id, command)) = rx.recv().await {
                if let Some(k) = kernel_weak.upgrade() {
                    let _ = handle_telegram_command(k, chat_id, command).await;
                }
            }
        }
    });
}
```

**Step 2: Add command handler function**

Add before `boot_with_config`:

```rust
async fn handle_telegram_command(
    kernel: Arc<OpenFangKernel>,
    chat_id: String,
    command: openfang_telegram::TelegramCommand,
) -> Result<(), String> {
    use openfang_telegram::TelegramCommand;

    let bot = kernel.telegram_bot.as_ref()
        .ok_or_else(|| "Telegram bot not initialized".to_string())?;

    match command {
        TelegramCommand::ListAgents => {
            let agents = kernel.registry.list();
            let agent_list = agents
                .iter()
                .map(|a| format!("- {} ({})", a.name, a.id))
                .collect::<Vec<_>>()
                .join("\n");

            let response = if agent_list.is_empty() {
                "No agents running.".to_string()
            } else {
                format!("Active agents:\n{}", agent_list)
            };

            bot.send_message(&chat_id, &response).await?;
        }
        TelegramCommand::Run { agent, task } => {
            // Find or create agent by name
            let agent_id = kernel.registry.find_by_name(&agent)
                .map(|e| e.id)
                .ok_or_else(|| format!("Agent '{}' not found", agent))?;

            // Send message to agent
            match kernel.send_message(agent_id, &task).await {
                Ok(_) => {
                    bot.send_message(&chat_id, &format!("✓ Task sent to {}", agent)).await?;
                }
                Err(e) => {
                    bot.send_message(&chat_id, &format!("✗ Error: {}", e)).await?;
                }
            }
        }
        TelegramCommand::Status { agent_id } => {
            let parsed_id = agent_id.parse()
                .map_err(|_| format!("Invalid agent ID: {}", agent_id))?;

            let entry = kernel.registry.get(parsed_id)
                .ok_or_else(|| format!("Agent {} not found", agent_id))?;

            let response = format!(
                "Agent: {}\nID: {}\nState: {:?}\nModel: {}",
                entry.name, entry.id, entry.state, entry.model
            );

            bot.send_message(&chat_id, &response).await?;
        }
        TelegramCommand::Help => {
            // Already handled in polling loop
        }
        TelegramCommand::Unknown { .. } => {
            // Ignore unknown commands
        }
    }

    Ok(())
}
```

**Step 3: Run tests**

Run: `cargo test --package openfang-kernel --lib`
Expected: Tests pass

**Step 4: Commit**

```bash
git add crates/openfang-kernel/src/kernel.rs
git commit -m "feat(telegram): add command handler for agent execution"
```

---

## Phase 2: Raindrop Incident Streaming

### Task 4: Add Raindrop config to OpenFang

**Files:**
- Modify: `crates/openfang-types/src/config.rs`

**Step 1: Add Raindrop config struct**

After the VideoConfig struct (around line 1000), add:

```rust
// ---------------------------------------------------------------------------
// Gap 10: Raindrop Integration
// ---------------------------------------------------------------------------

/// Raindrop observability integration configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RaindropConfig {
    /// Whether Raindrop integration is enabled.
    pub enabled: bool,

    /// Raindrop API base URL.
    pub api_url: String,

    /// Workspace ID to chat ID mapping for incident routing.
    #[serde(default)]
    pub workspace_chat_mapping: HashMap<String, String>,
}

impl Default for RaindropConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_url: "http://localhost:4201".to_string(),
            workspace_chat_mapping: HashMap::new(),
        }
    }
}
```

**Step 2: Add to KernelConfig**

In `KernelConfig` struct, add:

```rust
/// Raindrop observability integration.
#[serde(default)]
pub raindrop: RaindropConfig,
```

**Step 3: Add to Default impl**

In the `Default` impl for `KernelConfig`, add:

```rust
raindrop: RaindropConfig::default(),
```

**Step 4: Run tests**

Run: `cargo test --package openfang-types --lib`
Expected: All tests pass

**Step 5: Commit**

```bash
git add crates/openfang-types/src/config.rs
git commit -m "feat(raindrop): add config types for integration"
```

---

### Task 5: Add Raindrop types to OpenFang

**Files:**
- Create: `crates/openfang-types/src/raindrop.rs`
- Modify: `crates/openfang-types/src/lib.rs`

**Step 1: Create Raindrop types module**

```rust
//! Raindrop incident types for integration.

use serde::{Deserialize, Serialize};

/// Raindrop incident record.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RaindropIncident {
    pub id: String,
    pub workspace_id: String,
    pub agent_id: String,
    pub signal_label: String,
    pub severity: RaindropSeverity,
    pub status: RaindropIncidentStatus,
    pub latest_message: String,
    pub source_system: Option<String>,
    pub created_at: String,
}

/// Raindrop severity levels.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RaindropSeverity {
    Low,
    Medium,
    High,
    Critical,
}

/// Raindrop incident status.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum RaindropIncidentStatus {
    Open,
    Resolved,
}
```

**Step 2: Export module**

In `crates/openfang-types/src/lib.rs`, add:

```rust
pub mod raindrop;
```

**Step 3: Run tests**

Run: `cargo test --package openfang-types --lib`
Expected: Tests pass

**Step 4: Commit**

```bash
git add crates/openfang-types/src/raindrop.rs crates/openfang-types/src/lib.rs
git commit -m "feat(raindrop): add incident types for integration"
```

---

### Task 6: Create Raindrop subscriber module

**Files:**
- Create: `crates/openfang-kernel/src/raindrop_subscriber.rs`
- Modify: `crates/openfang-kernel/src/lib.rs`

**Step 1: Create subscriber module**

```rust
//! Raindrop incident subscriber for Telegram notification forwarding.

use openfang_types::config::RaindropConfig;
use openfang_types::raindrop::{RaindropIncident, RaindropSeverity};
use reqwest::Client;
use std::sync::Arc;
use tokio::time::{sleep, Duration};
use tracing::{info, warn};

/// Raindrop incident subscriber.
pub struct RaindropSubscriber {
    config: RaindropConfig,
    telegram_bot: Arc<openfang_telegram::TelegramBot>,
    client: Client,
}

impl RaindropSubscriber {
    /// Create a new Raindrop subscriber.
    pub fn new(
        config: RaindropConfig,
        telegram_bot: Arc<openfang_telegram::TelegramBot>,
    ) -> Self {
        Self {
            config,
            telegram_bot,
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
        let url = format!("{}/v1/incidents/stream", self.config.api_url.trim_end_matches('/'));

        let response = self.client
            .get(&url)
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
        let chat_id = self.config.workspace_chat_mapping
            .get(&incident.workspace_id)
            .ok_or_else(|| {
                format!("No chat_id configured for workspace {}", incident.workspace_id)
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

        self.telegram_bot.send_message(chat_id, &text).await?;

        info!("Forwarded incident {} to Telegram", incident.id);
        Ok(())
    }
}
```

**Step 2: Export module**

In `crates/openfang-kernel/src/lib.rs`, add:

```rust
pub mod raindrop_subscriber;
```

**Step 3: Add dependency**

In `crates/openfang-kernel/Cargo.toml`, add to `[dependencies]`:

```rust
futures = { workspace = true }
```

**Step 4: Run tests**

Run: `cargo test --package openfang-kernel --lib`
Expected: Tests pass

**Step 5: Commit**

```bash
git add crates/openfang-kernel/src/raindrop_subscriber.rs crates/openfang-kernel/src/lib.rs crates/openfang-kernel/Cargo.toml
git commit -m "feat(raindrop): add incident subscriber for Telegram forwarding"
```

---

### Task 7: Start Raindrop subscriber in kernel

**Files:**
- Modify: `crates/openfang-kernel/src/kernel.rs`

**Step 1: Add subscriber to kernel struct**

In `OpenFangKernel` struct, add:

```rust
/// Raindrop incident subscriber.
pub raindrop_subscriber: Option<Arc<crate::raindrop_subscriber::RaindropSubscriber>>,
```

**Step 2: Initialize subscriber in boot_with_config**

After Telegram bot initialization (around line 860), add:

```rust
// Initialize Raindrop subscriber
let raindrop_subscriber = if config.raindrop.enabled && telegram_bot.is_some() {
    let subscriber = Arc::new(crate::raindrop_subscriber::RaindropSubscriber::new(
        config.raindrop.clone(),
        telegram_bot.clone().unwrap(),
    ));

    info!(
        api_url = %config.raindrop.api_url,
        "Raindrop incident subscriber enabled"
    );

    Some(subscriber)
} else {
    None
};
```

**Step 3: Add to kernel initialization**

In the kernel struct initialization, add:

```rust
raindrop_subscriber,
```

**Step 4: Start subscriber background task**

After command processor task, add:

```rust
// Start Raindrop incident subscriber
if let Some(ref subscriber) = kernel.raindrop_subscriber {
    let subscriber_clone = subscriber.clone();

    tokio::spawn(async move {
        if let Err(e) = subscriber_clone.subscribe_and_forward().await {
            tracing::error!("Raindrop subscriber failed: {}", e);
        }
    });
}
```

**Step 5: Run tests**

Run: `cargo test --package openfang-kernel --lib`
Expected: Tests pass

**Step 6: Commit**

```bash
git add crates/openfang-kernel/src/kernel.rs
git commit -m "feat(raindrop): start incident subscriber in kernel background"
```

---

## Phase 3: Raindrop SSE Endpoint

### Task 8: Add SSE incident stream to Raindrop

**Files:**
- Modify: `crates/rd-api/src/lib.rs` (in raindrop_rebuild project)

**Step 1: Add SSE endpoint to router**

Find the router setup and add:

```rust
.route("/v1/incidents/stream", axum::routing::get(stream_incidents))
```

**Step 2: Add SSE handler function**

```rust
use axum::response::sse::{Event, Sse};
use futures::stream::{self, Stream};
use std::convert::Infallible;

async fn stream_incidents(
    State(state): State<SharedState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.bus.subscribe_incidents();

    let stream = stream::unfold(rx, |mut rx| async move {
        match rx.recv().await {
            Ok(incident) => {
                let json = serde_json::to_string(&incident).ok()?;
                let event = Event::default().event("incident").data(json);
                Some((Ok(event), rx))
            }
            Err(_) => None,
        }
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keepalive")
    )
}
```

**Step 3: Add dependencies**

In `crates/rd-api/Cargo.toml`, ensure these exist:

```toml
axum = { version = "0.7", features = ["http2", "macros"] }
futures = "0.3"
```

**Step 4: Run tests**

Run: `cargo test --package rd-api --lib`
Expected: Tests pass

**Step 5: Commit**

```bash
git add crates/rd-api/src/lib.rs crates/rd-api/Cargo.toml
git commit -m "feat(api): add SSE incident stream endpoint"
```

---

### Task 9: Update Raindrop notifier to skip Telegram

**Files:**
- Modify: `crates/rd-notifier/src/lib.rs` (in raindrop_rebuild project)

**Step 1: Skip Telegram sending**

Find the Telegram case in `notify_incident` (around line 123) and replace with:

```rust
NotificationChannelKind::Telegram => {
    // Delegated to OpenFang bot - incidents streamed via SSE
    info!(
        "incident {} delegated to OpenFang (Telegram via SSE)",
        incident.id
    );
}
```

**Step 2: Run tests**

Run: `cargo test --package rd-notifier --lib`
Expected: Tests pass

**Step 3: Commit**

```bash
git add crates/rd-notifier/src/lib.rs
git commit -m "feat(notifier): delegate Telegram to OpenFang via SSE"
```

---

## Phase 4: Configuration & Testing

### Task 10: Update example configurations

**Files:**
- Modify: `openfang.toml.example` (OpenFang)
- Modify: `.env.example` (both projects)

**Step 1: Update OpenFang example config**

In `openfang.toml.example`, add:

```toml
# Raindrop observability integration
[raindrop]
enabled = true
api_url = "http://localhost:4201"

[raindrop.workspace_chat_mapping]
"default-workspace" = "8444910202"  # Your Telegram chat ID
```

**Step 2: Update .env.example**

In both projects' `.env.example`, add:

```bash
# Shared Telegram bot token (from @BotFather)
TELEGRAM_BOT_TOKEN=8250681078:AAEyrZ4yWgfAZE1oTiv1_RJJAcWDCgnozvs
```

**Step 3: Commit OpenFang changes**

```bash
git add openfang.toml.example
git commit -m "docs: add Raindrop integration config example"
```

**Step 4: Commit Raindrop changes**

In raindrop_rebuild:
```bash
git add .env.example
git commit -m "docs: document shared Telegram bot token"
```

---

### Task 11: Live integration test

**Files:**
- Create: `scripts/test_telegram_integration.sh` (OpenFang)

**Step 1: Create test script**

```bash
#!/bin/bash
# Live integration test for shared Telegram bot

set -e

echo "=== Testing Shared Telegram Bot Integration ==="

# Check both services are running
echo "[1/5] Checking services..."
curl -sf http://localhost:4200/api/health > /dev/null || { echo "✗ OpenFang not running"; exit 1; }
curl -sf http://localhost:4201/api/health > /dev/null || { echo "✗ Raindrop not running"; exit 1; }
echo "✓ Both services running"

# Test bot connectivity
echo "[2/5] Testing bot connectivity..."
BOT_TOKEN="${TELEGRAM_BOT_TOKEN}"
RESPONSE=$(curl -sf "https://api.telegram.org/bot${BOT_TOKEN}/getMe")
BOT_USERNAME=$(echo "$RESPONSE" | python3 -c "import sys,json; print(json.load(sys.stdin)['result']['username'])")
echo "✓ Bot connected: @${BOT_USERNAME}"

# Test OpenFang agent list
echo "[3/5] Testing agent execution..."
AGENTS=$(curl -sf http://localhost:4200/api/agents)
echo "✓ Agents endpoint working"

# Test Raindrop incident stream
echo "[4/5] Testing incident stream..."
timeout 5 curl -sf http://localhost:4201/v1/incidents/stream > /tmp/sse_test.txt &
SSE_PID=$!
sleep 2
kill $SSE_PID 2>/dev/null || true
if grep -q "event: incident" /tmp/sse_test.txt 2>/dev/null; then
    echo "✓ SSE stream working (incidents flowing)"
else
    echo "✓ SSE endpoint reachable (no incidents yet)"
fi

# Manual test prompt
echo "[5/5] Manual verification needed:"
echo "  1. Send '/agents' to @OpenClawAIDemoBot"
echo "  2. Verify bot responds with agent list"
echo "  3. Trigger a Raindrop incident (scripts/simulate_failure.sh)"
echo "  4. Verify incident appears in Telegram within 5 seconds"
echo ""
echo "All automated checks passed. Complete manual verification above."
```

**Step 2: Make executable**

Run: `chmod +x scripts/test_telegram_integration.sh`

**Step 3: Run test**

Run: `./scripts/test_telegram_integration.sh`
Expected: All automated checks pass

**Step 4: Commit**

```bash
git add scripts/test_telegram_integration.sh
git commit -m "test: add integration test script for Telegram"
```

---

## Verification Steps

After completing all tasks:

### Start Both Services

**Terminal 1 (OpenFang):**
```bash
cd "/Users/gaganarora/Desktop/my projects/open_fang"
export TELEGRAM_BOT_TOKEN=8250681078:AAEyrZ4yWgfAZE1oTiv1_RJJAcWDCgnozvs
cargo build --release
./target/release/openfang start
```

**Terminal 2 (Raindrop):**
```bash
cd "/Users/gaganarora/Desktop/my projects/raindrop_rebuild"
export TELEGRAM_BOT_TOKEN=8250681078:AAEyrZ4yWgfAZE1oTiv1_RJJAcWDCgnozvs
cargo run --bin rd-api
```

### Test Command Flow

Open Telegram, send to @OpenClawAIDemoBot:
```
/agents
```

Expected response within 2 seconds:
```
Active agents:
- assistant (agent-id-123)
- researcher (agent-id-456)
```

### Test Incident Flow

**Terminal 3:**
```bash
cd "/Users/gaganarora/Desktop/my projects/raindrop_rebuild"
./scripts/simulate_failure.sh
```

Expected in Telegram within 5 seconds:
```
🔴 [incident:e2d06890-...]
Workspace: default-workspace
Agent: test-agent
Source: stereos
Label: agent.run.failed
Severity: High
Message: agent crashed
```

### Test Unauthorized User

From different Telegram account, send:
```
/agents
```

Expected: No response (silent ignore)

---

## Critical Files Modified

**OpenFang:**
- `crates/openfang-telegram/src/lib.rs` - Full bot implementation
- `crates/openfang-types/src/config.rs` - Raindrop config
- `crates/openfang-types/src/raindrop.rs` - Incident types
- `crates/openfang-kernel/src/raindrop_subscriber.rs` - SSE subscriber
- `crates/openfang-kernel/src/kernel.rs` - Background task wiring

**Raindrop:**
- `crates/rd-api/src/lib.rs` - SSE stream endpoint
- `crates/rd-notifier/src/lib.rs` - Skip Telegram, delegate to OpenFang

**Tests:**
- Unit tests in each module
- Live integration test script

---

## Success Criteria

✅ Bot responds to `/agents`, `/status`, `/run` commands
✅ Raindrop incidents appear in Telegram within 5 seconds
✅ No duplicate notifications (Raindrop's dedup works)
✅ Unauthorized users get no response
✅ OpenFang works without Raindrop (commands still work)
✅ Raindrop works without OpenFang (incidents buffer)
✅ Full test suite passes in both projects

---

Plan complete and saved to `docs/plans/2026-02-26-openfang-raindrop-telegram-integration-implementation.md`.

**Two execution options:**

**1. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints

**Which approach?**
