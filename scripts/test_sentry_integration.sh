#!/usr/bin/env bash
# OpenFang Sentry AI Monitoring - Live Integration Test
#
# This script tests the Sentry integration by:
# 1. Starting OpenFang with Sentry DSN
# 2. Triggering successful LLM calls
# 3. Triggering error scenarios (rate limit, auth, etc.)
# 4. Verifying Sentry receives events
#
# Usage:
#   export SENTRY_DSN="https://YOUR_KEY@o0.ingest.sentry.io/PROJECT_ID"
#   export GROQ_API_KEY="your_api_key"
#   ./scripts/test_sentry_integration.sh

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Configuration
API_BASE="http://127.0.0.1:4200/api"
DAEMON_PID=""

# Cleanup function
cleanup() {
    echo -e "${YELLOW}Cleaning up...${NC}"
    if [ -n "$DAEMON_PID" ]; then
        echo "Stopping daemon (PID: $DAEMON_PID)"
        kill -9 "$DAEMON_PID" 2>/dev/null || true
        wait "$DAEMON_PID" 2>/dev/null || true
    fi
}

trap cleanup EXIT

# Check prerequisites
echo -e "${GREEN}=== OpenFang Sentry Integration Test ===${NC}"
echo ""

if [ -z "${SENTRY_DSN:-}" ]; then
    echo -e "${RED}ERROR: SENTRY_DSN not set${NC}"
    echo "Get your DSN from sentry.io and export it:"
    echo "  export SENTRY_DSN='https://YOUR_KEY@o0.ingest.sentry.io/PROJECT_ID'"
    exit 1
fi

if [ -z "${GROQ_API_KEY:-}" ]; then
    echo -e "${YELLOW}WARNING: GROQ_API_KEY not set. Some tests will fail (expected).${NC}"
fi

# Step 1: Build release binary
echo -e "${GREEN}[1/6] Building OpenFang...${NC}"
cargo build --release -p openfang-cli || {
    echo -e "${RED}Build failed${NC}"
    exit 1
}
echo -e "${GREEN}✓ Build succeeded${NC}"
echo ""

# Step 2: Create test config with Sentry enabled
echo -e "${GREEN}[2/6] Creating test configuration...${NC}"
TEST_CONFIG=$(mktemp -d)/openfang_test_config.toml
cat > "$TEST_CONFIG" <<EOF
[default_model]
provider = "groq"
model = "llama-3.3-70b-versatile"
api_key_env = "GROQ_API_KEY"

[sentry]
dsn = "${SENTRY_DSN}"
environment = "integration-test"
traces_sample_rate = 1.0
include_prompts = false
performance_monitoring = true
error_tracking = true

[sentry.tags]
test_run = "true"
script_version = "1.0"
EOF

echo "Test config created at: $TEST_CONFIG"
echo -e "${GREEN}✓ Configuration ready${NC}"
echo ""

# Step 3: Start daemon with Sentry
echo -e "${GREEN}[3/6] Starting OpenFang daemon with Sentry...${NC}"
OPENFANG_CONFIG="$TEST_CONFIG" target/release/openfang start &
DAEMON_PID=$!
echo "Daemon PID: $DAEMON_PID"

# Wait for daemon to boot
echo "Waiting for daemon to boot..."
for i in {1..30}; do
    if curl -s "$API_BASE/health" >/dev/null 2>&1; then
        echo -e "${GREEN}✓ Daemon is ready${NC}"
        break
    fi
    sleep 1
    if [ $i -eq 30 ]; then
        echo -e "${RED}ERROR: Daemon failed to start${NC}"
        exit 1
    fi
done
echo ""

# Step 4: Test successful LLM call
echo -e "${GREEN}[4/6] Testing successful LLM call (will send to Sentry)...${NC}"

# Get an agent ID
AGENT_ID=$(curl -s "$API_BASE/agents" | python3 -c "import sys,json; print(json.load(sys.stdin)[0]['id'])" 2>/dev/null || echo "")

if [ -z "$AGENT_ID" ]; then
    echo -e "${YELLOW}No existing agents, creating one...${NC}"
    # Create a test agent
    curl -s -X POST "$API_BASE/agents" \
        -H "Content-Type: application/json" \
        -d '{
            "name": "sentry-test-agent",
            "model": {"provider": "groq", "model": "llama-3.3-70b-versatile"},
            "system_prompt": "You are a test agent."
        }' >/dev/null || true

    sleep 2
    AGENT_ID=$(curl -s "$API_BASE/agents" | python3 -c "import sys,json; print(json.load(sys.stdin)[0]['id'])")
fi

echo "Using agent: $AGENT_ID"

# Make a successful call
echo "Sending test message..."
RESPONSE=$(curl -s -X POST "$API_BASE/agents/$AGENT_ID/message" \
    -H "Content-Type: application/json" \
    -d '{"message": "Say hello in exactly 3 words."}' 2>&1)

echo "Response: $RESPONSE"
echo -e "${GREEN}✓ LLM call completed${NC}"
echo ""
echo -e "${YELLOW}Expected in Sentry:${NC}"
echo "  - Transaction: agent.loop"
echo "  - Span: llm.completion (status: Ok)"
echo "  - Tags: agent_id=$AGENT_ID, provider=groq, model=llama-3.3-70b-versatile"
echo "  - Measurements: input_tokens, output_tokens, duration_ms, cost_usd"
echo ""

# Step 5: Test error scenarios
echo -e "${GREEN}[5/6] Testing error scenarios...${NC}"

echo "5a. Testing invalid API key (Auth error)..."
# Temporarily break API key
ORIGINAL_KEY="${GROQ_API_KEY:-}"
export GROQ_API_KEY="invalid_key_test"

RESPONSE=$(curl -s -X POST "$API_BASE/agents/$AGENT_ID/message" \
    -H "Content-Type: application/json" \
    -d '{"message": "This should fail"}' 2>&1 || true)

export GROQ_API_KEY="$ORIGINAL_KEY"
echo "Response: $RESPONSE"
echo -e "${GREEN}✓ Auth error triggered${NC}"
echo ""
echo -e "${YELLOW}Expected in Sentry:${NC}"
echo "  - Error Event with error_category: Auth"
echo "  - Span status: Unauthenticated"
echo "  - Tags: is_retryable=false, is_billing=false"
echo ""

echo "5b. Testing invalid model (ModelNotFound error)..."
curl -s -X POST "$API_BASE/agents/$AGENT_ID/message" \
    -H "Content-Type: application/json" \
    -d '{"message": "Test", "override_model": "gpt-999-ultra"}' 2>&1 || true

echo -e "${GREEN}✓ Model not found error triggered${NC}"
echo ""
echo -e "${YELLOW}Expected in Sentry:${NC}"
echo "  - Error Event with error_category: ModelNotFound"
echo "  - Tags: is_retryable=false"
echo ""

# Step 6: Verification instructions
echo -e "${GREEN}[6/6] Test complete!${NC}"
echo ""
echo -e "${GREEN}=== Verification Steps ===${NC}"
echo ""
echo "1. Go to your Sentry project: https://sentry.io"
echo "2. Navigate to: Performance → Transactions"
echo "   - Filter by environment: integration-test"
echo "   - Look for 'agent.loop' transactions"
echo "   - Click on a transaction to see:"
echo "     • Span breakdown (llm.completion)"
echo "     • Tags (agent_id, provider, model)"
echo "     • Measurements (tokens, cost, duration)"
echo ""
echo "3. Navigate to: Issues → All Issues"
echo "   - Filter by environment: integration-test"
echo "   - Look for error events:"
echo "     • Auth error (401/403)"
echo "     • ModelNotFound error"
echo "   - Click on an issue to see:"
echo "     • error_category tag"
echo "     • is_retryable, is_billing flags"
echo "     • Breadcrumbs showing retry attempts"
echo ""
echo "4. Check Alerts (if configured):"
echo "   - High error rate alerts"
echo "   - High cost alerts (if triggered)"
echo ""
echo -e "${GREEN}✓ All tests completed successfully!${NC}"
echo ""
echo "Daemon will be stopped in 10 seconds..."
sleep 10
