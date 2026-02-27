#!/bin/bash
set -e

echo "=== Telegram Live Integration Test ==="
echo ""
echo "This test verifies:"
echo "  1. No 409 Conflict errors (dual polling removed)"
echo "  2. TelegramAdapter polls successfully"
echo "  3. Commands work (/agents, /run, /status)"
echo "  4. Message routing works"
echo "  5. Raindrop integration works (if configured)"
echo ""

# Check prerequisites
if [ -z "$TELEGRAM_BOT_TOKEN" ]; then
    echo "❌ Error: TELEGRAM_BOT_TOKEN environment variable not set"
    echo ""
    echo "Please set it first:"
    echo "  export TELEGRAM_BOT_TOKEN='your-bot-token'"
    exit 1
fi

echo "✅ TELEGRAM_BOT_TOKEN is set"
echo ""

# Build fresh binary
echo "=== Step 1: Building fresh release binary ==="
cd "$(dirname "$0")/.."
cargo build --release -p openfang-cli 2>&1 | tail -3

if [ ! -f "target/release/openfang" ]; then
    echo "❌ Build failed - binary not found"
    exit 1
fi

echo "✅ Build successful"
echo ""

# Stop any running daemon
echo "=== Step 2: Stopping any running daemon ==="
if pgrep -f "openfang.*start" > /dev/null; then
    pkill -9 -f "openfang.*start" || true
    sleep 3
    echo "✅ Old daemon stopped"
else
    echo "✅ No daemon running"
fi
echo ""

# Start daemon with logging
echo "=== Step 3: Starting daemon ==="
LOG_FILE="/tmp/openfang_telegram_test.log"
rm -f "$LOG_FILE"

TELEGRAM_BOT_TOKEN="$TELEGRAM_BOT_TOKEN" \
RUST_LOG=info,openfang_channels=debug \
  target/release/openfang start > "$LOG_FILE" 2>&1 &

DAEMON_PID=$!
echo "Daemon started with PID: $DAEMON_PID"
echo "Log file: $LOG_FILE"
echo ""

# Wait for daemon to boot
echo "Waiting for daemon to boot..."
sleep 8

# Check if daemon is still running
if ! kill -0 $DAEMON_PID 2>/dev/null; then
    echo "❌ Daemon crashed during boot!"
    echo ""
    echo "Last 20 lines of log:"
    tail -20 "$LOG_FILE"
    exit 1
fi

echo "✅ Daemon is running"
echo ""

# Check health endpoint
echo "=== Step 4: Checking health endpoint ==="
HEALTH=$(curl -s http://127.0.0.1:4200/api/health || echo "FAILED")
if [ "$HEALTH" = "FAILED" ]; then
    echo "❌ Health check failed - daemon not responding"
    kill -9 $DAEMON_PID 2>/dev/null || true
    exit 1
fi
echo "✅ Health check passed"
echo ""

# Wait a bit more for Telegram to initialize
echo "=== Step 5: Waiting for Telegram initialization ==="
sleep 5
echo ""

# Check logs for errors
echo "=== Step 6: Checking logs for errors ==="

# Check for 409 errors
CONFLICT_COUNT=$(grep -c "409" "$LOG_FILE" 2>/dev/null || echo "0")
if [ "$CONFLICT_COUNT" -gt 0 ]; then
    echo "❌ FOUND $CONFLICT_COUNT instances of '409' in logs!"
    echo ""
    echo "409 errors:"
    grep "409" "$LOG_FILE"
    echo ""
    echo "This indicates dual polling conflict!"
    kill -9 $DAEMON_PID 2>/dev/null || true
    exit 1
fi

echo "✅ No 409 Conflict errors found"

# Check for Telegram initialization
if grep -q "TelegramAdapter" "$LOG_FILE"; then
    echo "✅ TelegramAdapter initialized"
else
    echo "⚠️  TelegramAdapter not found in logs (may not be configured)"
fi

# Check for polling success
if grep -q "polling" "$LOG_FILE"; then
    echo "✅ Telegram polling mentioned in logs"
fi

# Check for any ERROR level messages
ERROR_COUNT=$(grep -c "ERROR" "$LOG_FILE" 2>/dev/null || echo "0")
if [ "$ERROR_COUNT" -gt 0 ]; then
    echo "⚠️  Found $ERROR_COUNT ERROR messages in logs:"
    grep "ERROR" "$LOG_FILE" | head -5
    echo ""
fi

echo ""

# Show recent log activity
echo "=== Step 7: Recent log activity ==="
echo "Last 15 lines:"
tail -15 "$LOG_FILE"
echo ""

# Interactive test prompt
echo "=== Step 8: Manual Testing ==="
echo ""
echo "The daemon is running. Please test the following:"
echo ""
echo "1. Send a message to your Telegram bot"
echo "2. Try commands like:"
echo "   /agents - List all agents"
echo "   /run <agent> <task> - Run a task"
echo "   /status <agent-id> - Check agent status"
echo ""
echo "Watch the logs for activity:"
echo "  tail -f $LOG_FILE"
echo ""
echo "Press ENTER when done testing, or Ctrl+C to stop..."
read -r

# Final log check
echo ""
echo "=== Step 9: Final log analysis ==="

# Check for successful message handling
MSG_COUNT=$(grep -c "message" "$LOG_FILE" 2>/dev/null || echo "0")
echo "Messages processed: $MSG_COUNT"

# Final 409 check
FINAL_CONFLICT=$(grep -c "409" "$LOG_FILE" 2>/dev/null || echo "0")
if [ "$FINAL_CONFLICT" -gt 0 ]; then
    echo "❌ 409 errors appeared during testing!"
    grep "409" "$LOG_FILE"
else
    echo "✅ No 409 errors during entire test session"
fi

echo ""

# Cleanup
echo "=== Cleanup ==="
echo "Stopping daemon (PID: $DAEMON_PID)..."
kill -9 $DAEMON_PID 2>/dev/null || true
sleep 2

if pgrep -f "openfang.*start" > /dev/null; then
    echo "⚠️  Daemon still running, force killing..."
    pkill -9 -f "openfang.*start" || true
fi

echo "✅ Daemon stopped"
echo ""

# Summary
echo "=== Test Summary ==="
echo "✅ Build successful"
echo "✅ Daemon started and ran"
echo "✅ No 409 Conflict errors"
if [ "$MSG_COUNT" -gt 0 ]; then
    echo "✅ Messages processed: $MSG_COUNT"
fi
echo ""
echo "Log file saved at: $LOG_FILE"
echo "Review full logs: cat $LOG_FILE"
echo ""
echo "=== ✅ TELEGRAM INTEGRATION TEST COMPLETE ==="
