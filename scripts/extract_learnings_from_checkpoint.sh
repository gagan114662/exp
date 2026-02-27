#!/bin/bash
# Extract learnings from Entire.io checkpoint and update MEMORY.md
#
# Usage: ./scripts/extract_learnings_from_checkpoint.sh

set -e

echo "=== Extract Learnings from Entire.io Checkpoint ==="

# Check if entire is available
if ! command -v entire &> /dev/null; then
    echo "❌ Entire CLI not found"
    exit 1
fi

# Get current session info
SESSION_INFO=$(entire status 2>&1)
echo "Current session status:"
echo "$SESSION_INFO"
echo ""

# Checkout checkpoint branch
echo "Checking checkpoint branch for latest session..."
git fetch origin entire/checkpoints/v1:entire/checkpoints/v1 2>/dev/null || true
git checkout entire/checkpoints/v1 2>/dev/null || echo "No checkpoints branch yet"

# Find latest session
LATEST_SESSION=$(ls -t sessions/ 2>/dev/null | head -1)

if [ -z "$LATEST_SESSION" ]; then
    echo "❌ No session found"
    git checkout main
    exit 1
fi

echo "Latest session: $LATEST_SESSION"
echo ""

# Return to main branch
git checkout main

echo "=== Review Checkpoint and Extract Learnings ==="
echo ""
echo "1. Review session transcript:"
echo "   git checkout entire/checkpoints/v1"
echo "   cat sessions/$LATEST_SESSION/transcript.md"
echo ""
echo "2. Look for patterns:"
echo "   - Bugs that were fixed"
echo "   - Solutions that worked"
echo "   - Mistakes that were made"
echo ""
echo "3. Update MEMORY.md:"
echo "   Edit ~/.claude/projects/-Users-gaganarora-Desktop-my-projects-open-fang/memory/MEMORY.md"
echo "   Add: Problem, Solution, Pattern, Files, Validation"
echo ""
echo "4. Keep MEMORY.md under 200 lines"
echo "   Move details to topic files if needed"
echo ""

echo "✅ Instructions displayed. Manual review and extraction needed."
