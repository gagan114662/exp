#!/bin/bash
set -e

echo "=== Channel Config Type System Live Integration Test ==="

# Stop any running daemon
echo "Stopping any running daemon..."
pkill -9 openfang || true
sleep 3

# Backup config
echo "Backing up config..."
mkdir -p ~/.openfang
if [ -f ~/.openfang/config.toml ]; then
    cp ~/.openfang/config.toml ~/.openfang/config.toml.backup
fi

# Build release binary
echo "Building release binary..."
cargo build --release -p openfang-cli

# Start daemon
echo "Starting daemon..."
GROQ_API_KEY=${GROQ_API_KEY} target/release/openfang start &
DAEMON_PID=$!
sleep 6

# Wait for health check
echo "Waiting for API to be ready..."
for i in {1..10}; do
    if curl -s http://127.0.0.1:4200/api/health >/dev/null 2>&1; then
        echo "API is ready"
        break
    fi
    if [ $i -eq 10 ]; then
        echo "❌ FAIL: API failed to start"
        kill -9 $DAEMON_PID || true
        exit 1
    fi
    sleep 2
done

# Test 1: WhatsApp webhook_port (should write as integer)
echo ""
echo "Test 1: WhatsApp webhook_port (integer)"
curl -s -X PUT "http://127.0.0.1:4200/api/channels/whatsapp/configure" \
  -H "Content-Type: application/json" \
  -d '{"webhook_port": "8443", "verify_token": "test123", "access_token": "abc"}'

sleep 1

# Check TOML for correct integer type
if grep -q "webhook_port = 8443" ~/.openfang/config.toml; then
    echo "✅ WhatsApp webhook_port written as integer (8443)"
    whatsapp_ok=true
elif grep -q 'webhook_port = "8443"' ~/.openfang/config.toml; then
    echo "❌ WhatsApp webhook_port written as string (\"8443\")"
    echo "TOML content:"
    grep webhook_port ~/.openfang/config.toml || echo "Field not found"
    whatsapp_ok=false
else
    echo "⚠️ WhatsApp webhook_port not found in config"
    whatsapp_ok=false
fi

# Test 2: Reddit subreddits (should write as array)
echo ""
echo "Test 2: Reddit subreddits (array)"
curl -s -X PUT "http://127.0.0.1:4200/api/channels/reddit/configure" \
  -H "Content-Type: application/json" \
  -d '{"client_id": "test_client", "client_secret": "test_secret", "subreddits": "rust,programming,opensource"}'

sleep 1

# Check TOML for correct array type
if grep -q 'subreddits = \["rust", "programming", "opensource"\]' ~/.openfang/config.toml; then
    echo "✅ Reddit subreddits written as array"
    reddit_ok=true
elif grep -q 'subreddits = "rust,programming,opensource"' ~/.openfang/config.toml; then
    echo "❌ Reddit subreddits written as string"
    echo "TOML content:"
    grep subreddits ~/.openfang/config.toml || echo "Field not found"
    reddit_ok=false
else
    echo "⚠️ Reddit subreddits not found in config"
    reddit_ok=false
fi

# Test 3: Teams allowed_tenants (should write as array)
echo ""
echo "Test 3: Teams allowed_tenants (array)"
curl -s -X PUT "http://127.0.0.1:4200/api/channels/teams/configure" \
  -H "Content-Type: application/json" \
  -d '{"app_id": "test_app", "app_password": "test_pass", "allowed_tenants": "tenant1,tenant2"}'

sleep 1

if grep -q 'allowed_tenants = \["tenant1", "tenant2"\]' ~/.openfang/config.toml; then
    echo "✅ Teams allowed_tenants written as array"
    teams_ok=true
elif grep -q 'allowed_tenants = "tenant1,tenant2"' ~/.openfang/config.toml; then
    echo "❌ Teams allowed_tenants written as string"
    echo "TOML content:"
    grep allowed_tenants ~/.openfang/config.toml || echo "Field not found"
    teams_ok=false
else
    echo "⚠️ Teams allowed_tenants not found in config"
    teams_ok=false
fi

# Test 4: Line webhook_port (should write as integer)
echo ""
echo "Test 4: Line webhook_port (integer)"
curl -s -X PUT "http://127.0.0.1:4200/api/channels/line/configure" \
  -H "Content-Type: application/json" \
  -d '{"channel_secret": "test_secret", "channel_access_token": "test_token", "webhook_port": "9443"}'

sleep 1

if grep -q "webhook_port = 9443" ~/.openfang/config.toml; then
    echo "✅ Line webhook_port written as integer (9443)"
    line_ok=true
elif grep -q 'webhook_port = "9443"' ~/.openfang/config.toml; then
    echo "❌ Line webhook_port written as string (\"9443\")"
    echo "TOML content:"
    grep webhook_port ~/.openfang/config.toml | grep -v whatsapp || echo "Field not found"
    line_ok=false
else
    echo "⚠️ Line webhook_port not found in config"
    line_ok=false
fi

# Cleanup
echo ""
echo "Cleaning up..."
kill -9 $DAEMON_PID || true
sleep 2

# Restore backup
if [ -f ~/.openfang/config.toml.backup ]; then
    mv ~/.openfang/config.toml.backup ~/.openfang/config.toml
fi

# Verify results
echo ""
echo "=== Test Results ==="
all_ok=true

if [ "$whatsapp_ok" = true ]; then
    echo "✅ WhatsApp: webhook_port as integer"
else
    echo "❌ WhatsApp: webhook_port type incorrect"
    all_ok=false
fi

if [ "$reddit_ok" = true ]; then
    echo "✅ Reddit: subreddits as array"
else
    echo "❌ Reddit: subreddits type incorrect"
    all_ok=false
fi

if [ "$teams_ok" = true ]; then
    echo "✅ Teams: allowed_tenants as array"
else
    echo "❌ Teams: allowed_tenants type incorrect"
    all_ok=false
fi

if [ "$line_ok" = true ]; then
    echo "✅ Line: webhook_port as integer"
else
    echo "❌ Line: webhook_port type incorrect"
    all_ok=false
fi

echo ""
if [ "$all_ok" = true ]; then
    echo "✅ ALL TESTS PASSED: Channel config types are correct"
    exit 0
else
    echo "❌ SOME TESTS FAILED: Channel config type system needs fixes"
    exit 1
fi
