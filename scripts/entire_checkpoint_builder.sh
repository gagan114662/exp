#!/bin/bash
# OpenFang Build Checkpoint Integration with Entire.io
#
# This script captures build context and creates checkpoints via entire.io CLI
# Usage: ./scripts/entire_checkpoint_builder.sh <command>

set -e

COMMAND=${1:-build}
TIMESTAMP=$(date +%Y-%m-%d_%H-%M-%S)
CHECKPOINT_DIR=".entire/checkpoints"
SESSION_FILE=".entire/current_session.json"

# Ensure directories exist
mkdir -p "$CHECKPOINT_DIR"

echo "=== OpenFang Build Checkpoint - $TIMESTAMP ==="
echo "Command: $COMMAND"

# Capture pre-build state
echo "Capturing pre-build state..."
PRE_BUILD_HASH=$(git rev-parse HEAD)
PRE_BUILD_STATUS=$(git status --short | wc -l)

# Execute build command based on input
case "$COMMAND" in
    build)
        echo "Running: cargo build --workspace --lib"
        BUILD_OUTPUT=$(cargo build --workspace --lib 2>&1)
        BUILD_EXIT=$?
        ;;
    test)
        echo "Running: cargo test --workspace"
        BUILD_OUTPUT=$(cargo test --workspace 2>&1)
        BUILD_EXIT=$?
        ;;
    clippy)
        echo "Running: cargo clippy --workspace --all-targets -- -D warnings"
        BUILD_OUTPUT=$(cargo clippy --workspace --all-targets -- -D warnings 2>&1)
        BUILD_EXIT=$?
        ;;
    release)
        echo "Running: cargo build --release -p openfang-cli"
        BUILD_OUTPUT=$(cargo build --release -p openfang-cli 2>&1)
        BUILD_EXIT=$?
        ;;
    *)
        echo "Unknown command: $COMMAND"
        echo "Usage: $0 {build|test|clippy|release}"
        exit 1
        ;;
esac

# Capture post-build state
POST_BUILD_HASH=$(git rev-parse HEAD)
POST_BUILD_STATUS=$(git status --short | wc -l)

# Create checkpoint metadata
CHECKPOINT_ID=$(echo "$TIMESTAMP-$COMMAND" | md5 | cut -c1-12)
CHECKPOINT_FILE="$CHECKPOINT_DIR/checkpoint_${CHECKPOINT_ID}.json"

cat > "$CHECKPOINT_FILE" << CHECKPOINT_EOF
{
  "checkpoint_id": "$CHECKPOINT_ID",
  "timestamp": "$TIMESTAMP",
  "command": "$COMMAND",
  "exit_code": $BUILD_EXIT,
  "pre_build": {
    "commit": "$PRE_BUILD_HASH",
    "modified_files": $PRE_BUILD_STATUS
  },
  "post_build": {
    "commit": "$POST_BUILD_HASH",
    "modified_files": $POST_BUILD_STATUS
  },
  "build_output_lines": $(echo "$BUILD_OUTPUT" | wc -l),
  "success": $([ $BUILD_EXIT -eq 0 ] && echo "true" || echo "false")
}
CHECKPOINT_EOF

# Save detailed build output
echo "$BUILD_OUTPUT" > "$CHECKPOINT_DIR/build_output_${CHECKPOINT_ID}.log"

# Create checkpoint via entire CLI (if commit was made during build)
if [ "$POST_BUILD_HASH" != "$PRE_BUILD_HASH" ]; then
    echo "Commit detected - creating Entire.io checkpoint..."
    entire status || true
fi

# Report results
if [ $BUILD_EXIT -eq 0 ]; then
    echo ""
    echo "✅ BUILD SUCCESS"
    echo "Checkpoint: $CHECKPOINT_ID"
    echo "Metadata: $CHECKPOINT_FILE"
    echo ""
else
    echo ""
    echo "❌ BUILD FAILED (exit code: $BUILD_EXIT)"
    echo "Checkpoint: $CHECKPOINT_ID"
    echo "Metadata: $CHECKPOINT_FILE"
    echo "Build log: $CHECKPOINT_DIR/build_output_${CHECKPOINT_ID}.log"
    echo ""
    exit $BUILD_EXIT
fi
