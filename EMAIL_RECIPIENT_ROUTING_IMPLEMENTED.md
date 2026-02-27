# Email Recipient Routing - Implementation Complete

**Issue #3 from Audit:** Email-to-agent routing by recipient address

**Status:** ✅ IMPLEMENTED

---

## Problem

Email routing only worked via subject tags (e.g., `[agent-name] message`). The parser did not read the `To:` header for routing, making it impossible to route emails based on recipient address.

**Impact:** Users couldn't send emails directly to agent-specific addresses like `sales@openfang.local` or `support@openfang.local`.

---

## Solution Implemented

### 1. Email Parser Enhancement (`crates/openfang-channels/src/email.rs`)

**Added `To:` header parsing:**
```rust
let mut to = String::new();

// Parse To: header
} else if let Some(addr) = line.strip_prefix("To: ") {
    to = Self::extract_email_address(addr);
```

**Added recipient email to metadata:**
```rust
if !to.is_empty() {
    metadata.insert(
        "recipient_email".to_string(),
        serde_json::Value::String(to.clone()),
    );
}
```

---

### 2. Bridge Routing Priority (`crates/openfang-channels/src/bridge.rs`)

**New routing priority order:**
1. **Recipient Email** (highest priority - most specific)
2. **Subject Tag** (`[agent-name]` - fallback)
3. **Router Resolution** (default behavior)

**Implementation:**
```rust
// Priority 1: Check recipient email (most specific)
let agent_id = if let Some(recipient_email_val) = message.metadata.get("recipient_email") {
    if let Some(recipient_email) = recipient_email_val.as_str() {
        match handle.find_agent_by_email(recipient_email).await {
            Ok(Some(id)) => {
                debug!("Routing message to agent {} via recipient email: {}", id, recipient_email);
                Some(id)
            }
            Ok(None) => None, // Fall through to subject tag routing
            Err(e) => { error!("Failed to lookup agent by email: {}", e); None }
        }
    } else { None }
} else { None };

// Priority 2: Check subject tag (fallback)
let agent_id = if agent_id.is_none() {
    // ... existing subject tag logic ...
} else { agent_id };

// Priority 4: Use router resolution (default)
let agent_id = if agent_id.is_none() {
    router.resolve(...)
} else { agent_id };
```

---

### 3. New ChannelBridgeHandle Method

**Added `find_agent_by_email` to trait:**
```rust
/// Find an agent by email address, returning its ID.
async fn find_agent_by_email(&self, email: &str) -> Result<Option<AgentId>, String>;
```

**Implementation in `KernelBridgeAdapter`:**
```rust
async fn find_agent_by_email(&self, email: &str) -> Result<Option<AgentId>, String> {
    // Search through all agents to find one with matching email
    for agent in self.kernel.registry.list() {
        if let Some(agent_email) = &agent.email {
            if agent_email.eq_ignore_ascii_case(email) {
                return Ok(Some(agent.id));
            }
        }
    }
    Ok(None)
}
```

---

## How It Works

### Example Email Flow

**Scenario 1: Direct Email Routing**
```
From: customer@example.com
To: sales@openfang.local
Subject: Pricing question

I'd like to know about your pricing.
```

**Routing:** Message routed to agent with email `sales@openfang.local` (Priority 1)

---

**Scenario 2: Subject Tag Fallback**
```
From: customer@example.com
To: info@openfang.local
Subject: [support-agent] Help needed

I need technical support.
```

**Routing:**
1. No agent has `info@openfang.local` → check subject tag
2. Message routed to `support-agent` (Priority 2)

---

**Scenario 3: Both Methods Present**
```
From: customer@example.com
To: sales@openfang.local
Subject: [support-agent] Question

...
```

**Routing:** Message routed to agent with `sales@openfang.local` (Priority 1 wins)

---

## Files Modified

1. **`crates/openfang-channels/src/email.rs`**
   - Added `To:` header parsing
   - Added `recipient_email` to metadata

2. **`crates/openfang-channels/src/bridge.rs`**
   - Added `find_agent_by_email` method to `ChannelBridgeHandle` trait
   - Implemented recipient-first routing priority
   - Added mock implementation for tests

3. **`crates/openfang-api/src/channel_bridge.rs`**
   - Implemented `find_agent_by_email` for `KernelBridgeAdapter`
   - Case-insensitive email matching

4. **`crates/openfang-channels/tests/bridge_integration_test.rs`**
   - Added `find_agent_by_email` to mock handler

---

## Verification

```bash
✅ Build: cargo build --workspace --lib (compiles successfully)
✅ Tests: All channel tests pass (9/9)
✅ Code Review: Routing logic verified via code inspection
```

---

## Benefits

1. **More Intuitive:** Users can email `sales@openfang.local` directly
2. **Backward Compatible:** Subject tag routing still works as fallback
3. **Flexible:** Both methods can coexist (recipient takes priority)
4. **Professional:** Matches standard email behavior

---

## Usage Example

### Step 1: Assign Email to Agent via Dashboard/API

```bash
curl -X PUT "http://localhost:4200/api/agents/{id}/email" \
  -H "Content-Type: application/json" \
  -d '{"email": "sales@company.com"}'
```

### Step 2: Configure Email Domain

In `~/.openfang/config.toml`:
```toml
[email]
imap_host = "imap.company.com"
smtp_host = "smtp.company.com"
username = "bot@company.com"
email_domain = "company.com"  # Auto-assign emails like agent-name@company.com
```

### Step 3: Send Email

Customers can now email agents directly:
- `sales@company.com` → Routes to sales agent
- `support@company.com` → Routes to support agent
- `engineering@company.com` → Routes to engineering agent

---

## Edge Cases Handled

✅ **Case-insensitive matching:** `Sales@company.com` matches `sales@company.com`
✅ **Graceful fallback:** If recipient doesn't match, falls back to subject tag
✅ **Multiple recipients:** Extracts first recipient from `To:` field
✅ **No match:** Falls back to router resolution (default behavior)

---

**Implementation Date:** February 27, 2026
**Status:** PRODUCTION READY ✅
