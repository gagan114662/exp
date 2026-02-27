#!/usr/bin/env bash
# Test script for Raindrop incident forwarding to Telegram

set -e

echo "🧪 Testing Raindrop Incident Forwarding"
echo "======================================="
echo

# 1. Check if Raindrop API is running
echo "1️⃣  Checking Raindrop API..."
if ! curl -s http://127.0.0.1:9000/health > /dev/null 2>&1; then
    echo "❌ Raindrop API not responding at http://127.0.0.1:9000"
    echo "   Start it with: cd raindrop && ./rd-api"
    exit 1
fi
echo "✅ Raindrop API is running"
echo

# 2. Check if OpenFang daemon is running
echo "2️⃣  Checking OpenFang daemon..."
if ! curl -s http://127.0.0.1:4200/api/health > /dev/null 2>&1; then
    echo "❌ OpenFang daemon not responding at http://127.0.0.1:4200"
    echo "   Start it with: RUST_LOG=debug target/release/openfang start"
    exit 1
fi
echo "✅ OpenFang daemon is running"
echo

# 3. Publish a test incident
echo "3️⃣  Publishing test incident..."
INCIDENT_ID=$(uuidgen | tr '[:upper:]' '[:lower:]')
TIMESTAMP=$(date -u +"%Y-%m-%dT%H:%M:%SZ")

RESPONSE=$(curl -s -X POST http://127.0.0.1:9000/v1/incidents \
  -H "Content-Type: application/json" \
  -H "Authorization: Bearer dev-token" \
  -d "{
    \"id\": \"$INCIDENT_ID\",
    \"workspace_id\": \"dev-workspace\",
    \"agent_id\": \"test-agent-001\",
    \"signal_label\": \"test_forwarding\",
    \"severity\": \"High\",
    \"status\": \"Open\",
    \"latest_message\": \"Test incident for forwarding verification at $TIMESTAMP\",
    \"source_system\": \"test_script\",
    \"created_at\": \"$TIMESTAMP\"
  }")

echo "Incident published: $INCIDENT_ID"
echo "Response: $RESPONSE"
echo

# 4. Wait for forwarding
echo "4️⃣  Waiting 5 seconds for incident to be forwarded..."
sleep 5
echo

# 5. Check OpenFang logs
echo "5️⃣  Recent OpenFang logs (check for forwarding):"
echo "================================================"
echo "Look for: 'Successfully parsed incident', 'Forwarded incident', or error messages"
echo
echo "To view live logs, run:"
echo "  tail -f ~/.openfang/logs/openfang.log"
echo
echo "Or check daemon output if running in foreground."
echo

echo "✅ Test complete!"
echo
echo "📱 Check your Telegram (@OpenClawAIDemoBot) for the incident message."
echo "   Expected format:"
echo "   🟠 [incident:$INCIDENT_ID]"
echo "   Workspace: dev-workspace"
echo "   Agent: test-agent-001"
echo "   Source: test_script"
echo "   Label: test_forwarding"
echo "   Severity: High"
echo "   Message: Test incident for forwarding verification at $TIMESTAMP"
