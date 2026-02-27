#!/bin/bash
# Entire.io Checkpoint → Auto-Memory Learning Extraction Workflow
# This is what ACTUALLY prevents repeated mistakes
#
# Usage: Run this at the end of each session

set -e

PROJECT_DIR="/Users/gaganarora/Desktop/my projects/open_fang"
MEMORY_DIR="$HOME/.claude/projects/-Users-gaganarora-Desktop-my-projects-open-fang/memory"
MEMORY_FILE="$MEMORY_DIR/MEMORY.md"

cd "$PROJECT_DIR"

echo "╔════════════════════════════════════════════════════════════════╗"
echo "║   Checkpoint → Memory Workflow: Preventing Repeated Mistakes  ║"
echo "╚════════════════════════════════════════════════════════════════╝"
echo ""

# Step 1: Review what was accomplished
echo "📋 Step 1: Review Recent Commits"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
git log -3 --oneline --decorate
echo ""

# Step 2: Check entire.io session (when available)
echo "🔍 Step 2: Check Entire.io Session Status"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
entire status || echo "Entire.io status unavailable"
echo ""
echo "To view checkpoint after session ends:"
echo "  entire rewind"
echo ""

# Step 3: Extract learnings template
echo "📝 Step 3: Extract Learnings to MEMORY.md"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Ask yourself:"
echo ""
echo "1. What bugs were fixed?"
echo "   → Root cause?"
echo "   → Solution that worked?"
echo ""
echo "2. What patterns emerged?"
echo "   → Code pattern to reuse?"
echo "   → Anti-pattern to avoid?"
echo ""
echo "3. What mistakes were made?"
echo "   → What would prevent this next time?"
echo ""

# Step 4: Template for MEMORY.md
echo "Template for MEMORY.md update:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
cat << 'TEMPLATE'

### Pattern Name (1 validation)
**Pattern:** One-line description with key code snippet
**Why:** Root cause explanation
**Files:** `path/to/file.rs`
**When:** When to apply this pattern

TEMPLATE
echo ""

# Step 5: Check MEMORY.md size
echo "📊 Step 4: Check MEMORY.md Size"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
CURRENT_LINES=$(wc -l < "$MEMORY_FILE")
echo "Current size: $CURRENT_LINES lines (limit: 200)"
echo ""

if [ $CURRENT_LINES -le 200 ]; then
    echo "✅ MEMORY.md is within limit - will load completely in future sessions"
else
    echo "⚠️  MEMORY.md exceeds limit by $((CURRENT_LINES - 200)) lines"
    echo ""
    echo "Only first 200 lines load automatically!"
    echo ""
    echo "ACTION REQUIRED:"
    echo "1. Move detailed examples to $MEMORY_DIR/debugging.md"
    echo "2. Keep only essential one-liners in MEMORY.md"
    echo "3. Link to topic files for details"
fi

echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "Edit MEMORY.md now:"
echo "  vim $MEMORY_FILE"
echo ""
echo "Or open in your editor:"
echo "  code $MEMORY_FILE"
echo ""
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""
echo "✅ This workflow (not entire.io alone) prevents repeated mistakes!"
echo ""
