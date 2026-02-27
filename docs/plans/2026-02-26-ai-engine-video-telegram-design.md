# AI Engine Orchestration, Video Summaries, and Telegram Integration

**Date:** February 26, 2026
**Status:** Approved
**Author:** Claude Sonnet 4.5

## Context

This design adds three major capabilities to OpenFang:

1. **AI Engine Orchestration** - Multi-model task routing with automatic model selection
2. **Video Summary Generation** - Post-execution video demonstrations of agent work
3. **Telegram Bidirectional Integration** - Full two-way communication with Telegram bot

### Why These Features

**Problem:** Current OpenFang uses a single default model for all tasks. Some tasks (research, coding, image generation) perform better with specialized models. Users want visibility into agent actions (video proof) and want to control agents through messaging platforms.

**Goals:**
- Automatically route tasks to best-performing models while allowing manual override
- Generate visual proof of agent work for transparency and debugging
- Enable Telegram as both command interface and notification channel

**Approach:** Integrated core extension (single binary, reuse existing infrastructure)

## Architecture Overview

```
┌─────────────────────────────────────────────────────────┐
│                    OpenFang Kernel                      │
│                                                         │
│  ┌────────────────┐  ┌──────────────┐  ┌─────────────┐│
│  │ Model          │  │ Video        │  │ Telegram    ││
│  │ Orchestrator   │  │ Renderer     │  │ Channel     ││
│  └────────────────┘  └──────────────┘  └─────────────┘│
│         │                    │                 │       │
│         │                    │                 │       │
│  ┌──────▼──────────────────────────────────────▼─────┐ │
│  │         Existing Infrastructure                   │ │
│  │  - LLM Drivers  - Audit Log  - Event Bus         │ │
│  │  - Agent Loop   - Channels   - API Server        │ │
│  └────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────┘
```

## Component 1: Model Orchestrator

### Location
`crates/openfang-runtime/src/model_orchestrator.rs`

### Responsibility
Analyze task characteristics and route to optimal model while allowing manual overrides.

### Task Classification
```rust
enum TaskType {
    Research,       // Multi-step info gathering
    Coding,         // Code generation/debugging
    QuickQA,        // Simple questions
    ImageGen,       // Image creation
    LongContext,    // >10k tokens context
    Default,        // Fallback
}
```

### Selection Logic
1. Check if agent has routing rules (manual override)
2. If no override, classify task based on:
   - Keywords (e.g., "research", "code", "generate image")
   - Context length
   - Tool requirements
3. Map to best model from catalog
4. Fallback to user's default if model unavailable

### Routing Rules
```rust
pub struct RoutingRule {
    task_pattern: String,        // Regex or keyword
    provider: String,             // "anthropic", "gemini", etc.
    model: String,                // Model identifier
    priority: u8,                 // Higher = preferred
}
```

### Configuration
```toml
[orchestrator]
enabled = true

[orchestrator.routing]
research = { provider = "gemini", model = "gemini-2.0-flash-exp" }
coding = { provider = "anthropic", model = "claude-opus-4-6" }
quick = { provider = "groq", model = "llama-3-70b" }
image = { provider = "openai", model = "dall-e-3" }
```

### Integration Point
```rust
// Before (current):
let driver = create_driver(&config)?;

// After (with orchestration):
let task_type = classify_task(&request);
let driver = create_driver_with_orchestration(task_type, &config)?;
```

### Fallback Chain
```
Classified model unavailable
  → Try next best model for task type
    → Fall back to user's default model
      → Error if default unavailable
```

## Component 2: Video Renderer

### Location
`crates/openfang-runtime/src/video_renderer.rs`

### Responsibility
Generate visual summaries of agent execution by reading audit logs and rendering to MP4.

### Data Source
OpenFang's existing audit trail provides all execution events:
- Tool calls (shell, browser, file ops)
- LLM requests/responses
- Errors
- State changes

### Generation Flow
```
Agent completes task
  → VideoRenderer reads audit log
    → Filter key events (ignore noise)
      → Generate frames (HTML canvas)
        → Render to PNG using headless Chrome
          → Stitch to MP4 using ffmpeg
            → Store in ~/.openfang/recordings/
```

### Key Events to Visualize
- **Shell commands:** Command + stdout/stderr
- **Code edits:** Before/after diffs
- **Browser actions:** Screenshots
- **API calls:** Request/response
- **Errors:** Stack traces, error messages
- **Final output:** Task result

### Frame Format
```
┌─────────────────────────────────────────┐
│ Agent: researcher-001                   │
│ Task: Analyze Bitcoin trends            │
│ Step 3/7: web_search("Bitcoin Q1 2026")│
│                                         │
│ [Screenshot or terminal output]        │
│                                         │
│ Duration: 00:02:34                      │
│ Cost: $0.03                             │
└─────────────────────────────────────────┘
```

### Video Specs
- **Resolution:** 1280x720
- **Frame rate:** 2 FPS (slideshow)
- **Duration:** 30-60 seconds typical
- **Format:** MP4 (H.264 codec)
- **Storage:** `~/.openfang/recordings/{agent_id}/{task_id}.mp4`

### Dependencies
- `headless_chrome` crate for HTML → PNG
- `ffmpeg` system binary for PNG → MP4
- Falls back gracefully if ffmpeg missing (saves raw audit JSON)

### API Endpoint
```
GET /api/agents/{id}/recordings/{task_id}
  → Returns MP4 file or download URL
```

### Retention Policy
```toml
[video]
enabled = true
retention_days = 30        # Delete after 30 days
max_storage_mb = 5000      # Stop recording if exceeds 5GB
```

## Component 3: Telegram Channel

### Location
`crates/openfang-channels/src/telegram.rs`

### Responsibility
Bidirectional communication: receive commands from Telegram, send notifications back.

### Connection Method
Long-polling Telegram Bot API (`getUpdates` endpoint) running in background tokio task.

### Configuration
```toml
[telegram]
enabled = true
bot_token = "YOUR_TELEGRAM_BOT_TOKEN_HERE"
allowed_users = ["123456789", "987654321"]
rate_limit_per_minute = 10
```

### Incoming Commands (Telegram → OpenFang)

| Command | Action |
|---------|--------|
| `/run <agent> <task>` | Execute task on agent |
| `/agents` | List all agents |
| `/status <agent_id>` | Check agent status |
| `/recordings <task_id>` | Get video link |
| `/help` | Show available commands |
| Direct message | Route to default agent |

**Example Flow:**
```
User: /run researcher analyze Bitcoin trends
Bot: ⏳ Running researcher...
      [typing indicator while executing]
Bot: ✅ Task completed in 2m 34s

     Summary: Bitcoin showed 15% growth...

     [View Recording] [Full Report]
```

### Outgoing Notifications (OpenFang → Telegram)

Agents can send notifications based on events:

```rust
pub struct TelegramNotification {
    chat_id: String,
    message: String,
    parse_mode: Option<String>,  // "Markdown" or "HTML"
    reply_markup: Option<InlineKeyboard>,
}
```

**Configuration per agent:**
```toml
[agent.notifications]
telegram_chat_id = "123456789"
on_complete = true
on_error = true
on_insight = false
```

### Message Formatting
- **Markdown** for code blocks, bold, italics
- **Inline buttons** for actions (View Recording, Retry, Delete)
- **Long messages** (>4096 chars) split or sent as file
- **Images/videos** as attachments when available

### Security
- **Whitelist:** Only allowed user IDs can send commands
- **Rate limiting:** Max 10 commands/minute per user
- **Audit logging:** All commands logged to audit trail
- **Optional password:** For sensitive operations

### Error Handling
- Bot token invalid → fail at startup with clear error
- Network lost → reconnect with exponential backoff (1s, 2s, 4s, 8s, max 60s)
- API rate limit → queue messages, send when available
- Unauthorized user → reply "Access denied" + log attempt
- Message too long → split into chunks or send as TXT file

## Error Handling Strategy

### Model Orchestrator
- Model unavailable → fall back to default
- API key missing → skip to next option
- Rate limit → retry OR switch model
- Invalid routing config → log warning, use default

### Video Renderer
- ffmpeg missing → skip video, save audit JSON
- Rendering fails → save raw audit log
- Disk space low → cleanup old recordings
- Corrupted frame → skip, continue with others

### Telegram Channel
- Connection lost → reconnect (exponential backoff)
- Rate limit hit → queue messages
- Parse error → send helpful error + examples
- Command fails → reply with error, don't crash

**General Principles:**
- Graceful degradation (features fail independently)
- User-facing errors are friendly, not stack traces
- All errors logged to audit trail
- Critical failures trigger notifications

## Testing Strategy

### Model Orchestrator Tests
```rust
#[test]
fn test_task_classification() {
    assert_eq!(classify_task("research Bitcoin"), TaskType::Research);
    assert_eq!(classify_task("write code for auth"), TaskType::Coding);
}

#[test]
fn test_fallback_chain() {
    // Mock: Gemini unavailable
    // Expect: Falls back to default
}

#[test]
fn test_routing_override() {
    // Agent with routing rule
    // Expect: Uses specified model, not auto-selected
}
```

### Video Renderer Tests
```rust
#[test]
fn test_frame_generation() {
    let events = mock_audit_events();
    let frames = generate_frames(&events)?;
    assert!(frames.len() > 0);
}

#[test]
fn test_video_without_ffmpeg() {
    // Mock: ffmpeg not found
    // Expect: Saves JSON, logs warning
}
```

### Telegram Tests
```rust
#[test]
fn test_command_parsing() {
    assert_eq!(parse_command("/run agent1 task"), Command::Run { ... });
}

#[test]
fn test_unauthorized_user() {
    let msg = Message { user_id: "999999", ... };
    assert!(is_blocked(&msg));
}

#[test]
async fn test_reconnection() {
    // Mock: Network fails
    // Expect: Reconnects with backoff
}
```

### Integration Tests
- Full flow: Telegram → orchestrator → agent → video → Telegram
- Parallel execution (multiple users)
- Error recovery scenarios

### Performance Benchmarks
- Orchestrator overhead: <50ms per decision
- Video generation: async, non-blocking
- Telegram polling: handle 100+ msg/sec

## Implementation Files

**New files to create:**
- `crates/openfang-runtime/src/model_orchestrator.rs` (~300 LOC)
- `crates/openfang-runtime/src/video_renderer.rs` (~400 LOC)
- `crates/openfang-channels/src/telegram.rs` (~500 LOC)

**Files to modify:**
- `crates/openfang-runtime/src/drivers/mod.rs` (add orchestration wrapper)
- `crates/openfang-kernel/src/kernel.rs` (initialize new components)
- `crates/openfang-types/src/config.rs` (add config structs)
- `crates/openfang-api/src/routes.rs` (add video endpoint)

**Dependencies to add:**
```toml
[dependencies]
# Video rendering
headless_chrome = "1.0"

# Telegram
teloxide = { version = "0.12", default-features = false, features = ["macros", "ctrlc_handler", "auto-send"] }
```

## Rollout Plan

### Phase 1: Model Orchestrator (Week 1)
- Implement task classification
- Add routing logic
- Config validation
- Unit tests
- Integration with existing drivers

### Phase 2: Video Renderer (Week 2)
- Frame generation from audit log
- HTML canvas rendering
- ffmpeg integration
- Storage and cleanup
- API endpoint

### Phase 3: Telegram Channel (Week 3)
- Bot setup and authentication
- Command parsing
- Message handling
- Notification system
- Security and rate limiting

### Phase 4: Integration & Polish (Week 4)
- End-to-end testing
- Performance optimization
- Documentation
- Dashboard UI for recordings
- Example configs

## Success Metrics

- **Model Orchestrator:** 90%+ tasks routed to appropriate model
- **Video Renderer:** <30s generation time for typical task
- **Telegram:** <1s response time for commands
- **Reliability:** 99.9% uptime for Telegram polling
- **User Satisfaction:** Video recordings useful for debugging/demo

## Future Enhancements

- **Live streaming:** Real-time video as agent executes (vs post-execution)
- **Multi-bot support:** Connect multiple Telegram bots
- **Voice commands:** Telegram voice messages → speech-to-text → agent
- **Collaborative agents:** Multiple users controlling same agent via Telegram
- **Model cost tracking:** Per-model cost breakdown in orchestrator
