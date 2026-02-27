#!/bin/bash
set -e

echo "=== Agent Email Integration Test ==="
echo ""

# Configuration
OPENFANG_BIN="${OPENFANG_BIN:-target/release/openfang}"
API_BASE="${API_BASE:-http://127.0.0.1:4200}"
EMAIL_DOMAIN="${EMAIL_DOMAIN:-myagents.local}"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Helper functions
check_requirement() {
    if ! command -v "$1" &> /dev/null; then
        echo -e "${RED}✗ Required command not found: $1${NC}"
        exit 1
    fi
}

log_step() {
    echo -e "${YELLOW}▶ $1${NC}"
}

log_success() {
    echo -e "${GREEN}✓ $1${NC}"
}

log_error() {
    echo -e "${RED}✗ $1${NC}"
}

# Check requirements
log_step "Checking requirements..."
check_requirement jq
check_requirement curl
log_success "All requirements met"
echo ""

# Step 1: Build OpenFang
log_step "Building OpenFang (release mode)..."
if ! cargo build --release -p openfang-cli 2>&1 | tail -3; then
    log_error "Build failed"
    exit 1
fi
log_success "Build completed"
echo ""

# Step 2: Stop any running OpenFang daemon
log_step "Stopping any existing OpenFang daemon..."
pkill -f openfang || true
sleep 2
log_success "Cleanup complete"
echo ""

# Step 3: Configure email channel
log_step "Creating test configuration..."
CONFIG_DIR="$HOME/.openfang"
mkdir -p "$CONFIG_DIR"

cat > "$CONFIG_DIR/config.toml" << EOF
[channels.email]
imap_host = "localhost"
imap_port = 993
smtp_host = "localhost"
smtp_port = 587
username = "admin@${EMAIL_DOMAIN}"
password_env = "EMAIL_PASSWORD"
email_domain = "${EMAIL_DOMAIN}"
poll_interval_secs = 10
folders = ["INBOX"]
allowed_senders = []
default_agent = "assistant"
EOF

log_success "Configuration created"
echo ""

# Step 4: Start OpenFang daemon
log_step "Starting OpenFang daemon..."
EMAIL_PASSWORD="test123" "$OPENFANG_BIN" start &
DAEMON_PID=$!
echo "Daemon PID: $DAEMON_PID"
sleep 10

# Check if daemon is running
if ! kill -0 $DAEMON_PID 2>/dev/null; then
    log_error "Daemon failed to start"
    exit 1
fi
log_success "Daemon started"
echo ""

# Step 5: Wait for API to be ready
log_step "Waiting for API to be ready..."
MAX_RETRIES=30
RETRY_COUNT=0
while [ $RETRY_COUNT -lt $MAX_RETRIES ]; do
    if curl -s -f "$API_BASE/api/health" > /dev/null 2>&1; then
        break
    fi
    RETRY_COUNT=$((RETRY_COUNT + 1))
    sleep 1
done

if [ $RETRY_COUNT -eq $MAX_RETRIES ]; then
    log_error "API did not become ready in time"
    kill $DAEMON_PID 2>/dev/null || true
    exit 1
fi
log_success "API is ready"
echo ""

# Step 6: Create a test agent
log_step "Creating test agent..."
AGENT_RESPONSE=$(curl -s -X POST "$API_BASE/api/agents" \
    -H 'Content-Type: application/json' \
    -d '{
        "name": "test-email-agent",
        "model": "groq/llama-3.3-70b-versatile"
    }')

AGENT_ID=$(echo "$AGENT_RESPONSE" | jq -r '.id')
if [ -z "$AGENT_ID" ] || [ "$AGENT_ID" = "null" ]; then
    log_error "Failed to create agent"
    echo "Response: $AGENT_RESPONSE"
    kill $DAEMON_PID 2>/dev/null || true
    exit 1
fi
log_success "Agent created: $AGENT_ID"
echo ""

# Step 7: Verify email address was assigned
log_step "Checking agent email assignment..."
EMAIL_RESPONSE=$(curl -s "$API_BASE/api/agents/$AGENT_ID/email")
AGENT_EMAIL=$(echo "$EMAIL_RESPONSE" | jq -r '.email')

if [ -z "$AGENT_EMAIL" ] || [ "$AGENT_EMAIL" = "null" ]; then
    log_error "No email address assigned"
    echo "Response: $EMAIL_RESPONSE"
    kill $DAEMON_PID 2>/dev/null || true
    exit 1
fi

EXPECTED_EMAIL="test-email-agent@${EMAIL_DOMAIN}"
if [ "$AGENT_EMAIL" != "$EXPECTED_EMAIL" ]; then
    log_error "Email mismatch"
    echo "Expected: $EXPECTED_EMAIL"
    echo "Got: $AGENT_EMAIL"
    kill $DAEMON_PID 2>/dev/null || true
    exit 1
fi
log_success "Email assigned correctly: $AGENT_EMAIL"
echo ""

# Step 8: Test email endpoint error cases
log_step "Testing error cases..."

# Test invalid agent ID
INVALID_RESPONSE=$(curl -s -w "\n%{http_code}" "$API_BASE/api/agents/not-a-uuid/email")
INVALID_BODY=$(echo "$INVALID_RESPONSE" | head -1)
INVALID_STATUS=$(echo "$INVALID_RESPONSE" | tail -1)

if [ "$INVALID_STATUS" != "400" ]; then
    log_error "Invalid ID should return 400, got $INVALID_STATUS"
    kill $DAEMON_PID 2>/dev/null || true
    exit 1
fi
log_success "Invalid ID handled correctly (400)"

# Test non-existent agent
FAKE_UUID="00000000-0000-0000-0000-000000000000"
NOTFOUND_RESPONSE=$(curl -s -w "\n%{http_code}" "$API_BASE/api/agents/$FAKE_UUID/email")
NOTFOUND_STATUS=$(echo "$NOTFOUND_RESPONSE" | tail -1)

if [ "$NOTFOUND_STATUS" != "404" ]; then
    log_error "Non-existent agent should return 404, got $NOTFOUND_STATUS"
    kill $DAEMON_PID 2>/dev/null || true
    exit 1
fi
log_success "Non-existent agent handled correctly (404)"
echo ""

# Step 9: Cleanup
log_step "Cleaning up..."
kill $DAEMON_PID 2>/dev/null || true
sleep 2
log_success "Cleanup complete"
echo ""

echo -e "${GREEN}========================================${NC}"
echo -e "${GREEN}✓ All tests passed!${NC}"
echo -e "${GREEN}========================================${NC}"
echo ""
echo "Summary:"
echo "  - Agent created: $AGENT_ID"
echo "  - Email assigned: $AGENT_EMAIL"
echo "  - Email format validated"
echo "  - Error cases handled correctly"
echo ""
echo "Next steps:"
echo "  1. Set up Stalwart mail server"
echo "  2. Configure DNS (MX records)"
echo "  3. Send test email to $AGENT_EMAIL"
echo "  4. Verify agent receives message"
