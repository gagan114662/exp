#!/bin/bash
# Quick Sentry Test - Sends a test message and shows you what to look for

set -e

echo "🧪 Testing Sentry Integration..."
echo ""

# Check if OpenFang is running
if ! curl -s http://127.0.0.1:4200/api/health >/dev/null 2>&1; then
    echo "❌ OpenFang is not running!"
    echo "Start it with: openfang start"
    exit 1
fi

echo "✅ OpenFang is running"
echo ""

# Get first agent ID
echo "📡 Getting agent list..."
AGENT_ID=$(curl -s http://127.0.0.1:4200/api/agents | python3 -c "import sys,json; agents=json.load(sys.stdin); print(agents[0]['id'] if agents else '')" 2>/dev/null || echo "")

if [ -z "$AGENT_ID" ]; then
    echo "❌ No agents found. Create one first!"
    exit 1
fi

echo "✅ Using agent: $AGENT_ID"
echo ""

# Send test message
echo "📨 Sending test message to trigger Sentry event..."
RESPONSE=$(curl -s -X POST "http://127.0.0.1:4200/api/agents/$AGENT_ID/message" \
    -H "Content-Type: application/json" \
    -d '{"message": "Say hello in exactly 3 words"}' 2>&1)

echo "✅ Message sent!"
echo ""
echo "Response: $RESPONSE"
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "🎯 Now check your Sentry dashboard:"
echo ""
echo "1. Go to: https://sentry.io"
echo "2. Click on: openfang-monitoring project"
echo "3. Go to: Performance → Transactions"
echo "4. You should see: agent.loop transaction"
echo "5. Click on it to see:"
echo "   ├─ input_tokens"
echo "   ├─ output_tokens"
echo "   ├─ cost_usd"
echo "   ├─ duration"
echo "   └─ provider: gemini"
echo ""
echo "It may take 10-30 seconds to appear!"
echo ""
echo "Direct link:"
echo "https://sentry.io/organizations/foolish/performance/"
echo ""
