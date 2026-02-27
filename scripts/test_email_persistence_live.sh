#!/bin/bash
set -e

echo "=== Email Persistence Live Integration Test ==="

# Stop any running daemon
echo "Stopping any running daemon..."
pkill -9 openfang || true
sleep 3

# Backup DB
echo "Backing up database..."
mkdir -p ~/.openfang
if [ -f ~/.openfang/memory.db ]; then
    cp ~/.openfang/memory.db ~/.openfang/memory.db.backup
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

# Get first agent ID
echo "Getting agent ID..."
agent_id=$(curl -s http://127.0.0.1:4200/api/agents | python3 -c "import sys,json; agents=json.load(sys.stdin); print(agents[0]['id'] if agents else 'none')")

if [ "$agent_id" = "none" ]; then
    echo "❌ FAIL: No agents found"
    kill -9 $DAEMON_PID || true
    exit 1
fi

echo "Using agent: $agent_id"

# Assign email via API
echo "Assigning email via API..."
curl -s -X PUT "http://127.0.0.1:4200/api/agents/${agent_id}/email" \
  -H "Content-Type: application/json" \
  -d '{"email": "test-persistence@example.com"}'

# Verify email was assigned
sleep 1
email_before=$(curl -s http://127.0.0.1:4200/api/agents | python3 -c "import sys,json; agents=json.load(sys.stdin); agent=[a for a in agents if a['id']=='${agent_id}']; print(agent[0].get('email', 'missing') if agent else 'missing')")
echo "Email before restart: $email_before"

if [ "$email_before" != "test-persistence@example.com" ]; then
    echo "❌ FAIL: Email not assigned correctly before restart (got: $email_before)"
    kill -9 $DAEMON_PID || true
    [ -f ~/.openfang/memory.db.backup ] && mv ~/.openfang/memory.db.backup ~/.openfang/memory.db
    exit 1
fi

# RESTART DAEMON
echo "Restarting daemon to test persistence..."
kill -9 $DAEMON_PID
sleep 3

GROQ_API_KEY=${GROQ_API_KEY} target/release/openfang start &
DAEMON_PID=$!
sleep 6

# Wait for health check
for i in {1..10}; do
    if curl -s http://127.0.0.1:4200/api/health >/dev/null 2>&1; then
        echo "API restarted successfully"
        break
    fi
    if [ $i -eq 10 ]; then
        echo "❌ FAIL: API failed to restart"
        kill -9 $DAEMON_PID || true
        exit 1
    fi
    sleep 2
done

# Verify email persisted across restart
email_after=$(curl -s http://127.0.0.1:4200/api/agents | python3 -c "import sys,json; agents=json.load(sys.stdin); agent=[a for a in agents if a['id']=='${agent_id}']; print(agent[0].get('email', 'missing') if agent else 'missing')")
echo "Email after restart: $email_after"

# Cleanup
echo "Cleaning up..."
kill -9 $DAEMON_PID || true
sleep 2

# Restore backup
if [ -f ~/.openfang/memory.db.backup ]; then
    mv ~/.openfang/memory.db.backup ~/.openfang/memory.db
fi

# Verify result
if [ "$email_after" = "test-persistence@example.com" ]; then
    echo ""
    echo "✅ PASS: Email persisted across daemon restart"
    echo ""
    exit 0
else
    echo ""
    echo "❌ FAIL: Email was '$email_after', expected 'test-persistence@example.com'"
    echo ""
    exit 1
fi
