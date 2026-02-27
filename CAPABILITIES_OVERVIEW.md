# OpenFang - Complete Capabilities Overview

> **OpenFang Agent OS** - A Rust-based operating system for autonomous AI agents

---

## 🎯 Core Capabilities

### 1. **Multi-Agent System (14 Crates)**
- Spawn unlimited agents with unique identities
- Parent-child agent hierarchies
- Agent-to-agent communication (A2A protocol)
- Peer-to-peer agent networking (OFP wire protocol)
- Agent lifecycle management (spawn/kill/pause/resume)
- Permission-based operational modes
- Cross-agent memory sharing

### 2. **LLM Integration (122 Models, 22+ Providers)**

**Native Drivers:**
- Anthropic Claude (Opus, Sonnet, Haiku)
- Google Gemini (2.0 Flash, Pro, etc.)
- OpenAI (GPT-4, GPT-3.5, etc.)

**OpenAI-Compatible Providers (20+):**
- Groq, Ollama, LM Studio, vLLM
- Mistral AI, Cohere, Perplexity
- Together AI, Anyscale, DeepInfra
- Fireworks AI, Replicate, Modal
- Baseten, Hugging Face, AWS Bedrock
- Azure OpenAI, OpenRouter, and more

**Model Catalog Features:**
- 51 built-in model definitions
- Automatic provider detection
- Cost tracking per model
- Tier classification (flagship/workhorse/budget/local)
- Dynamic model routing

### 3. **Built-In Tools (30+ Tools)**

**File Operations:**
- `file_read` - Read files from workspace
- `file_write` - Write/create files
- `file_list` - List directory contents
- `apply_patch` - Multi-hunk diff patching

**Web & Search:**
- `web_fetch` - Fetch web pages (with SSRF protection)
- `web_search` - Multi-provider search (Tavily, Brave, Perplexity, DuckDuckGo)

**Command Execution:**
- `shell_exec` - Execute shell commands (sandboxed)

**Agent Management:**
- `agent_send` - Send messages to other agents
- `agent_spawn` - Create new agents
- `agent_list` - List running agents
- `agent_kill` - Terminate agents
- `agent_find` - Discover agents by name/tag/tool

**Memory & Knowledge:**
- `memory_store` - Persistent key-value storage
- `memory_recall` - Retrieve stored data
- `knowledge_add_entity` - Add to knowledge graph
- `knowledge_add_relation` - Create entity relationships
- `knowledge_query` - Query knowledge graph

**Task Management:**
- `task_post` - Post tasks to shared queue
- `task_claim` - Claim available tasks
- `task_complete` - Mark tasks done
- `task_list` - List queued tasks

**Scheduling:**
- `schedule_create` - Create cron jobs (natural language or cron syntax)
- `schedule_list` - List scheduled tasks
- `schedule_delete` - Remove schedules

**Events:**
- `event_publish` - Broadcast custom events to agent fleet

**Media Processing:**
- `image_analyze` - Describe images using vision models
- `media_describe` - Extract metadata from media files
- `media_transcribe` - Audio transcription
- `image_generate` - Image generation (via configured providers)
- `text_to_speech` - TTS synthesis

**RLM Tools (Recursive Language Model - Advanced):**
- `rlm_dataset_load` - Load datasets for analysis
- `rlm_js_eval` - Execute JavaScript via Bun runtime
- `rlm_fanout` - Adaptive parallel sub-LLM fanout with evidence filtering
- `rlm_state_inspect` - Inspect RLM execution state

---

## 📡 **Messaging Channels (40 Platforms)**

### Instant Messaging
1. **Telegram** - Bot API with webhook support
2. **Discord** - Gateway intents, slash commands
3. **Slack** - Socket Mode + REST API
4. **WhatsApp** - Via WA Web gateway
5. **Signal** - Signal CLI integration
6. **Matrix** - Homeserver federation
7. **Mattermost** - WebSocket + webhooks
8. **Rocket.Chat** - REST API + realtime
9. **Zulip** - Stream-based chat
10. **Twitch** - IRC + Helix API
11. **IRC** - Classic protocol
12. **XMPP/Jabber** - Federated messaging
13. **Mumble** - Voice chat protocol

### Enterprise Platforms
14. **Microsoft Teams** - Bot Framework v3
15. **Google Chat** - Chat API v1
16. **Webex** - Webex Teams API
17. **DingTalk** - Enterprise messaging
18. **Feishu/Lark** - ByteDance platform

### Email & SMS
19. **Email** - IMAP/SMTP with auto-assignment ✨ NEW
20. **SMS** - Twilio integration

### Social & Community
21. **Reddit** - Reddit API
22. **Mastodon** - ActivityPub protocol
23. **Bluesky** - AT Protocol
24. **LinkedIn** - LinkedIn API
25. **Twitter** - (via Twitter Hand)
26. **Nostr** - Decentralized protocol

### Collaboration
27. **Discourse** - Forum platform
28. **Gitter** - Developer chat
29. **Nextcloud** - Nextcloud Talk
30. **Guilded** - Gaming community platform

### Privacy-Focused
31. **Threema** - End-to-end encrypted
32. **Keybase** - Crypto messaging

### Productivity
33. **Flock** - Team messenger
34. **Twist** - Async team chat
35. **Pumble** - Workspace chat
36. **Revolt** - Open-source Discord alternative

### Notifications
37. **Ntfy** - Push notifications
38. **Gotify** - Self-hosted push

### Universal
39. **Webhook** - Generic HTTP webhooks
40. **WebChat** - Built-in web UI

**All channels support:**
- Bidirectional communication
- Thread support (where platform allows)
- Rate limiting (per-user GCRA)
- Format conversion (Markdown ↔ Platform-specific)
- DM/group policy enforcement
- Agent routing & bindings

---

## 🤖 **Hands (7 Autonomous Capability Packages)**

Hands are curated, autonomous capability packages that combine tools, skills, and specialized behaviors:

1. **Clip Hand** - Content curation and extraction
2. **Lead Hand** - Leadership and orchestration
3. **Collector Hand** - Data gathering and aggregation
4. **Predictor Hand** - Forecasting and prediction
5. **Researcher Hand** - Research and analysis
6. **Twitter Hand** - Social media management
7. **Browser Hand** - Web automation (Playwright)

---

## 🧩 **Extensions (25 Integration Templates)**

Pre-configured MCP server templates for popular services:
- AWS, GitHub, GitLab, Postgres, Slack, Google Drive
- File systems, databases, APIs, and more

---

## 🔒 **Security & Governance**

### Capability-Based Security (RBAC)
- Fine-grained permissions (tools, network, memory, spawning)
- Capability inheritance validation
- Network access control (domain allowlists)
- Memory isolation (self.*, team.*, global.*)
- Tool filtering per agent

### Execution Control
- Approval manager (require user confirmation)
- Execution policies (auto-approve/prompt/deny)
- WASM sandbox with fuel metering
- Command injection prevention
- Taint tracking for sensitive data

### Audit & Monitoring
- Merkle hash chain audit trail
- Usage event persistence
- Cost metering per agent
- Budget quotas (hourly/daily/monthly)
- Sentry integration for error tracking
- Heartbeat monitoring

---

## 💾 **Memory & Persistence**

### Memory Substrate (SQLite Schema v5)
- **Sessions:** Conversation history with message deduplication
- **KV Storage:** Agent-scoped key-value pairs
- **Knowledge Graph:** Entities and relations with confidence scores
- **Task Board:** Shared task queue across agents
- **Usage Tracking:** Per-agent cost and token usage
- **Canonical Sessions:** Cross-channel memory linking
- **Embeddings:** Vector similarity search (Ollama/OpenAI)

### Memory Management
- Automatic session compaction (LLM-based summarization)
- Confidence decay over time (configurable rate)
- Memory consolidation scheduler
- Session repair (history validation)

---

## 🌐 **Networking & Federation**

### OFP Wire Protocol
- Peer-to-peer agent networking
- HMAC-SHA256 mutual authentication
- Nonce-based replay protection
- Constant-time verification (timing-attack resistant)
- mDNS peer discovery
- Configurable bootstrap peers

### A2A Protocol (Agent-to-Agent)
- AgentCard discovery (/.well-known/agent.json)
- Task submission to external agents
- Status polling
- Cross-system agent collaboration

### MCP (Model Context Protocol)
- Client mode: Connect to external MCP servers
- Server mode: Expose OpenFang tools via MCP
- 25 pre-configured extension templates
- Stdio, SSE, and sequential transports
- Tool definition caching

---

## ⚙️ **Automation & Workflows**

### Workflow Engine
- Multi-step workflow orchestration
- Conditional branching
- Parallel execution
- Error handling & retries

### Trigger Engine
- Event-driven automation
- Pattern matching on events
- Proactive agent activation
- Scheduled triggers

### Cron Scheduler
- Natural language scheduling ("every 5 minutes", "daily at 9am")
- Traditional cron syntax support
- Per-agent cron jobs
- Delivery to last active channel

### Background Executor
- Long-running background agents
- Proactive agents (condition-based wake-up)
- Resource quotas

---

## 🎨 **User Interfaces**

### 1. **WebChat Dashboard (http://127.0.0.1:50051/)**
- Alpine.js SPA
- Multi-agent chat interface
- Real-time agent status
- Cost tracking display
- Budget management
- Model catalog browser
- Provider auth status
- Session management
- Configuration editor

### 2. **Desktop App (Tauri 2.0)**
- Native macOS/Windows/Linux app
- System tray integration
- Desktop notifications
- Single-instance enforcement
- Hide-to-tray on close
- Mobile-ready architecture

### 3. **CLI (Interactive & Scripting)**
```bash
# Agent management
openfang agent spawn <name>
openfang agent list
openfang agent chat <name>
openfang agent kill <id>

# Daemon control
openfang start
openfang status
openfang doctor

# Configuration
openfang init
openfang config show
openfang config edit

# Channels
openfang channel setup <name>
openfang channel list
openfang channel test <name>

# Skills
openfang skill install <name>
openfang skill search <query>
openfang skill create <name>

# Workflows & Triggers
openfang workflow create
openfang trigger list

# MCP Server Mode
openfang mcp
```

### 4. **REST API (76 Endpoints)**
- Agent CRUD operations
- Message sending (standard & streaming)
- Workflow management
- Memory operations
- Channel configuration
- Model catalog queries
- Provider management
- Budget tracking
- Health checks
- **NEW:** Email assignment queries

### 5. **WebSocket API**
- Real-time bidirectional chat
- `ws://127.0.0.1:50051/api/agents/{id}/ws`
- Message streaming
- Lifecycle events

### 6. **SSE Streaming**
- Server-Sent Events for streaming responses
- `POST /api/agents/{id}/message/stream`

---

## 🧠 **Advanced Features**

### RLM (Recursive Language Model)
- Bun-backed JavaScript runtime
- Adaptive parallel sub-LLM fanout
- Evidence-based filtering
- Provenance tracking
- State inspection
- Dataset loading
- Multi-stage reasoning

### Browser Automation
- Playwright integration
- Session management
- Viewport configuration
- Screenshot capture
- Page interaction

### Video Processing
- Video summary rendering
- Frame extraction
- Multi-modal understanding

### Process Management
- Persistent process manager
- REPL sessions (Python, Node.js, etc.)
- Long-running server processes
- Interactive shell sessions

---

## 📦 **Skill System (60+ Bundled Skills)**

**Skill Runtimes:**
- Python skills
- WASM skills
- Node.js skills
- Prompt-only skills

**Skill Features:**
- FangHub marketplace integration
- ClawHub.ai cross-ecosystem discovery
- Skill verification (SHA256 checksums)
- Prompt injection scanning
- Auto-installation from repos

**Bundled Skills Include:**
- Code generation, debugging, review
- Research and analysis
- Data processing
- API integration
- DevOps automation
- And 55+ more...

---

## 🏗️ **Developer Features**

### Migration Engine
- OpenClaw → OpenFang migration
- YAML → TOML conversion
- Tool name mapping (21 compatibility mappings)
- Provider name normalization
- Agent manifest import

### Setup Wizard
- Interactive configuration
- Provider authentication
- API key validation
- Model selection
- Channel setup

### Health Monitoring
- System diagnostics (`openfang doctor`)
- Provider health checks
- Extension health monitoring
- Latency tracking
- Error rate monitoring

---

## 📊 **Cost Management & Metering**

### Budget System
- Global budget quotas
- Per-agent budget tracking
- Hourly/daily/monthly limits
- Cost forecasting
- Alert thresholds
- Usage event persistence

### Metering Engine
- Token counting
- API call tracking
- Cost calculation (20+ model pricing catalogs)
- Per-agent cost ranking
- Cost attribution

### Rate Limiting
- GCRA (Generic Cell Rate Algorithm)
- Cost-aware rate limiting
- Per-user limits
- Per-channel limits
- Configurable windows

---

## 🔐 **Enterprise Features**

### Authentication & Authorization
- RBAC (Role-Based Access Control)
- Bearer token authentication
- User identity mapping
- Channel-specific auth
- Permission inheritance

### Device Management
- Device pairing
- Multi-device sync
- Pairing code generation
- Trusted device registry

### Compliance & Audit
- Merkle hash chain audit trail
- Action logging (spawn/kill/message/capability)
- Cryptographic verification
- Tamper detection
- Sentry error tracking (production-grade)

---

## 🎨 **Agent Customization**

### Visual Identity
- Emoji icons
- Avatar URLs
- Color schemes
- Archetype classification
- Personality vibes
- Greeting styles

### Behavior Configuration
- System prompts
- Temperature control
- Max tokens limits
- Streaming preferences
- Tool allowlists
- Model overrides per agent
- Custom capabilities

---

## 📮 **NEW: Agent Email System**

### Auto-Assignment
- Each agent gets `{agent-name}@{your-domain}.com`
- Automatic on agent creation
- Persistent across restarts
- Zero per-agent cost (self-hosted)

### Email Functionality
- IMAP polling (async)
- SMTP sending
- Subject-based routing: `[agent-name] message`
- Thread support (In-Reply-To headers)
- Sender filtering
- RFC822 parsing
- TLS/STARTTLS support

### Email Integration
- Works with any IMAP/SMTP server
- Recommended: Stalwart Mail Server
- API: `GET /api/agents/{id}/email`
- 348 tests covering email functionality

**Cost Savings:** $480-$4,180/month vs AgentMail SaaS

---

## 🚀 **Performance Features**

### Optimization
- Async/await throughout
- Connection pooling
- Result caching (web fetch - 15min TTL)
- Session compaction
- Memory consolidation
- Efficient JSON serialization

### Scalability
- Multi-threaded runtime (Tokio)
- DashMap for concurrent access
- Rate limiting prevents overload
- Resource quotas per agent
- Graceful degradation

---

## 🛠️ **Configuration System**

### Config Sources
- TOML-based (`~/.openfang/config.toml`)
- Environment variables
- Command-line overrides
- Web dashboard editor
- Type-safe deserialization with `#[serde(default)]`

### Configurable Aspects
- Default model & provider
- API server bind address
- Memory decay rate
- Network settings (OFP)
- Channel configurations (40 adapters)
- MCP servers
- Budget limits
- Session compaction thresholds
- Cron schedules
- Broadcast policies

---

## 📚 **Advanced Capabilities**

### Model Orchestration
- Multi-model task routing
- Intelligent model selection
- Fallback strategies
- Load balancing

### Video Rendering
- Video summary generation
- Frame analysis
- Multi-modal synthesis

### Auto-Reply Engine
- Pattern-based suppression
- Configurable rules
- Cross-channel support

### Broadcast System
- Multi-agent broadcasting
- Sequential or parallel strategies
- Response aggregation
- Team coordination

---

## 🔬 **Testing & Quality**

### Test Coverage
- **1,332 unit tests** across workspace
- Integration test scripts
- Live API testing
- Channel-specific test suites
- **348 email tests** (comprehensive)

### Quality Tooling
- Clippy linting (zero warnings enforced)
- Rustfmt formatting
- Build verification scripts
- CI/CD workflows (Harness integration)

---

## 📖 **Documentation**

### Available Docs
- Architecture overview (`docs/architecture.md`)
- CLI reference (`docs/cli-reference.md`)
- Configuration guide (`docs/configuration.md`)
- Harness engineering (`docs/harness-engineering.md`)
- Email implementation (`AGENT_EMAIL_IMPLEMENTATION.md`)
- Example configs (`openfang.toml.example`)

---

## 🎯 **Use Cases**

OpenFang excels at:

### 1. **Customer Support Automation**
- Multi-channel support (email, chat, social)
- 24/7 availability
- Intelligent routing
- Knowledge base integration

### 2. **Research & Analysis**
- Web search across multiple providers
- Knowledge graph building
- Report generation
- Data processing

### 3. **DevOps & Automation**
- Server monitoring
- Deployment automation
- Log analysis
- Incident response

### 4. **Personal Assistant**
- Email management
- Calendar scheduling
- Task tracking
- Information lookup

### 5. **Development Workflows**
- Code review agents
- Documentation writers
- Testing assistants
- Architecture planners

### 6. **Team Coordination**
- Multi-agent collaboration
- Task distribution
- Status reporting
- Decision support

---

## 💡 **Unique Differentiators**

1. **Truly Open Source** - Apache-2.0/MIT dual license
2. **Self-Hosted** - No vendor lock-in, full control
3. **Multi-Model** - 122 models from 22+ providers
4. **40 Channels** - Most comprehensive channel support
5. **Production-Grade** - 1,332 tests, security hardening
6. **Cost-Effective** - Self-hosted = $20/month vs $500+/month SaaS
7. **Rust Performance** - Fast, memory-safe, concurrent
8. **Extensible** - Skills, MCP, A2A protocols
9. **Enterprise-Ready** - RBAC, audit trails, metering
10. **Email per Agent** - Unique self-hosted email system ✨

---

## 🎓 **Architecture Highlights**

### Clean Separation
```
CLI/Desktop → API → Kernel → Runtime/Channels/Memory
                        ↓
                    Types (shared foundation)
```

### No Circular Dependencies
- `KernelHandle` trait enables inter-agent tools
- Clean dependency graph
- Easy to extend and maintain

### Production Hardening
- SSRF protection in web fetch
- Command injection prevention
- Timing-attack resistance (crypto)
- Rate limiting
- Input validation
- Error recovery

---

## 📈 **Scale**

**Current Stats:**
- **14 crates** (now including openfang-telegram)
- **76 REST endpoints**
- **40 channel adapters**
- **30+ tools**
- **60 bundled skills**
- **25 extension templates**
- **7 hands**
- **122 models supported**
- **1,332 tests**
- **~50,000+ lines of Rust code**

---

## 🚀 **What You Can Do Right Now**

```bash
# 1. Initialize OpenFang
openfang init

# 2. Start the daemon
openfang start

# 3. Open dashboard
open http://127.0.0.1:50051/

# 4. Create an agent (gets auto-assigned email!)
openfang agent spawn my-assistant

# 5. Chat with your agent
openfang agent chat my-assistant

# 6. Check agent's email
curl http://127.0.0.1:50051/api/agents/{id}/email

# 7. Set up a channel (e.g., Telegram)
openfang channel setup telegram

# 8. Install a skill
openfang skill install researcher

# 9. Create a workflow
openfang workflow create

# 10. Schedule automation
curl -X POST http://127.0.0.1:50051/api/cron \
  -d '{"description": "Daily report", "schedule": "daily at 9am"}'
```

---

## 🎯 **In Summary**

**OpenFang is a complete Agent Operating System that:**
- ✅ Runs multiple AI agents simultaneously
- ✅ Connects to 40+ messaging platforms
- ✅ Supports 122 models from 22+ providers
- ✅ Provides 30+ built-in tools
- ✅ Includes 60 pre-built skills
- ✅ Enables peer-to-peer agent networking
- ✅ Offers enterprise-grade security
- ✅ Delivers comprehensive cost tracking
- ✅ **NEW:** Auto-assigns email addresses to every agent
- ✅ All self-hosted with zero vendor lock-in

**Think of it as:**
- **Kubernetes for AI agents** (orchestration, scaling, monitoring)
- **Unix for LLMs** (tools, pipes, composability)
- **Email server for agents** (every agent gets an inbox)
- **Operating system** (scheduling, memory, networking, security)

**Built with:** Rust, Tokio, Axum, SQLite, Tauri, Alpine.js

**Status:** Production-ready with 1,332 passing tests ✅
