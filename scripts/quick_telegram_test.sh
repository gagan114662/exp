#!/bin/bash

echo "=== Quick Telegram 409 Error Test ==="
echo ""

# Check for token in environment
if [ -z "$TELEGRAM_BOT_TOKEN" ]; then
    echo "ERROR: TELEGRAM_BOT_TOKEN environment variable not set"
    echo "Usage: TELEGRAM_BOT_TOKEN='your-token' $0"
    exit 1
fi

# Build if needed
if [ ! -f "target/release/openfang" ]; then
    echo "Building release binary (this will take a few minutes)..."
    cargo build --release -p openfang-cli
fi

echo "Starting daemon..."
LOG_FILE="/tmp/openfang_quick_test.log"
rm -f "$LOG_FILE"

# Kill any existing daemon
pkill -9 -f "openfang.*start" 2>/dev/null || true
sleep 2

# Start daemon
RUST_LOG=info,openfang_channels=debug \
  target/release/openfang start > "$LOG_FILE" 2>&1 &

DAEMON_PID=$!
echo "Daemon PID: $DAEMON_PID"
echo "Log: $LOG_FILE"
echo ""

# Wait for boot
echo "Waiting 15 seconds for boot..."
sleep 15

# Check if running
if ! kill -0 $DAEMON_PID 2>/dev/null; then
    echo "❌ Daemon crashed!"
    tail -30 "$LOG_FILE"
    exit 1
fi

echo "✅ Daemon is running"
echo ""

# Check for 409 errors
echo "Checking for 409 Conflict errors..."
CONFLICT_COUNT=$(grep -c "409" "$LOG_FILE" 2>/dev/null || echo "0")

if [ "$CONFLICT_COUNT" -gt 0 ]; then
    echo "❌ FOUND 409 ERRORS!"
    grep "409" "$LOG_FILE"
    kill -9 $DAEMON_PID
    exit 1
else
    echo "✅ No 409 errors found!"
fi

echo ""
echo "Telegram integration looks good:"
grep -i "telegram" "$LOG_FILE" | head -10

echo ""
echo "=== Test your bot now! ==="
echo "1. Go to: t.me/OpenClawAIDemoBot"
echo "2. Send: /agents"
echo "3. Watch logs: tail -f $LOG_FILE"
echo ""
echo "Press ENTER when done..."
read

# Final check
FINAL_409=$(grep -c "409" "$LOG_FILE" 2>/dev/null || echo "0")
if [ "$FINAL_409" -eq 0 ]; then
    echo "✅ SUCCESS - No 409 errors!"
else
    echo "❌ FAIL - Found 409 errors"
fi

# Cleanup
kill -9 $DAEMON_PID 2>/dev/null || true
echo ""
echo "Daemon stopped. Logs at: $LOG_FILE"
