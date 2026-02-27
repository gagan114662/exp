#!/bin/bash
# Run this at the end of each Claude Code session
# Extracts learnings and updates MEMORY.md

PROJECT_MEMORY="$HOME/.claude/projects/-Users-gaganarora-Desktop-my-projects-open-fang/memory/MEMORY.md"

echo "=== End Session Learning Extraction ==="
echo ""
echo "📝 What did this session accomplish?"
echo "   (Review recent commits)"
echo ""
git log -5 --oneline
echo ""
echo "🔍 Extract key learnings:"
echo ""
echo "1. What bugs were fixed?"
echo "   - Root cause?"
echo "   - Solution that worked?"
echo ""
echo "2. What patterns emerged?"
echo "   - Code pattern that should be reused?"
echo "   - Anti-pattern to avoid?"
echo ""
echo "3. Update MEMORY.md:"
echo ""
echo "   ### Pattern Name (1 validation)"
echo "   **Problem:** What went wrong"
echo "   **Solution:** What fixed it"  
echo "   **Pattern:** [code example]"
echo "   **Files:** Where this applies"
echo "   **Validation:** Today's session"
echo ""
echo "4. Current MEMORY.md size:"
wc -l "$PROJECT_MEMORY" | awk '{print "   " $1 " lines (limit: 200)"}'
echo ""

# Check if over limit
if [ $(wc -l < "$PROJECT_MEMORY") -gt 200 ]; then
    echo "⚠️  MEMORY.md is $(wc -l < "$PROJECT_MEMORY") lines (limit: 200)"
    echo ""
    echo "ACTION REQUIRED: Trim MEMORY.md"
    echo ""
    echo "Current sections:"
    grep "^## " "$PROJECT_MEMORY"
    echo ""
    echo "Suggestions:"
    echo "- Move detailed examples to memory/debugging.md"
    echo "- Keep only essential patterns in MEMORY.md"
    echo "- Link to topic files for details"
fi

echo ""
echo "✅ Ready to update MEMORY.md"
echo ""
echo "Edit now?"
echo "  vim $PROJECT_MEMORY"
