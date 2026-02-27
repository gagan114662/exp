#!/bin/bash
# Post-Session Learning Extraction Ritual
# Run this after completing significant work

set -e

PROJECT_MEMORY="$HOME/.claude/projects/-Users-gaganarora-Desktop-my-projects-open-fang/memory/MEMORY.md"

echo "=== Session Wrap-Up ==="
echo ""
echo "1. Review what you built:"
echo "   git diff HEAD~1..HEAD"
echo ""
echo "2. Check entire.io checkpoint:"
echo "   entire rewind"
echo ""
echo "3. Ask yourself:"
echo "   - What bugs did I fix?"
echo "   - What patterns worked?"
echo "   - What mistakes were made?"
echo "   - What would prevent this next time?"
echo ""
echo "4. Update MEMORY.md:"
echo "   vim $PROJECT_MEMORY"
echo ""
echo "   Add new pattern:"
echo "   ### Pattern Name (1 validation)"
echo "   **Problem:** What went wrong"
echo "   **Solution:** What fixed it"
echo "   **Pattern:** Code example"
echo "   **Files:** Where this applies"
echo "   **Validation:** This session"
echo ""
echo "5. Keep MEMORY.md focused:"
echo "   - First 200 lines load automatically"
echo "   - Move details to topic files if too long"
echo ""
echo "6. Current MEMORY.md size:"
wc -l "$PROJECT_MEMORY" | awk '{print "   " $1 " lines (limit: 200)"}'
echo ""

if [ $(wc -l < "$PROJECT_MEMORY") -gt 200 ]; then
    echo "⚠️  WARNING: MEMORY.md exceeds 200 lines!"
    echo "   Only first 200 lines load into Claude's context"
    echo "   Action: Move details to separate topic files"
fi

echo ""
echo "✅ Run this after every significant session to build institutional memory"
