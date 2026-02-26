#!/usr/bin/env bash
# Helper script to investigate lsm-tree repository structure

set -e

REPO_URL="https://github.com/input-output-hk/lsm-tree"
CLONE_DIR="./lsm-tree-investigation"

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "  LSM-Tree Repository Investigation"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo ""

# Clone if not exists
if [ ! -d "$CLONE_DIR" ]; then
    echo "📦 Cloning lsm-tree repository..."
    git clone "$REPO_URL" "$CLONE_DIR"
    echo ""
fi

cd "$CLONE_DIR"

echo "📂 Repository Structure:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
tree -L 2 -I 'dist*|.git' || find . -maxdepth 2 -type d | head -20
echo ""

echo "📦 Cabal Files:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
find . -name "*.cabal" -type f
echo ""

echo "📚 Exposed Modules:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
grep -r "exposed-modules:" --include="*.cabal" -A 10 | head -40
echo ""

echo "📝 Main Source Directories:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
find . -type d -name "Database" -o -name "src"
echo ""

echo "🔍 LSMTree Module Files:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
find . -path "*/Database/LSMTree*" -name "*.hs" | head -20
echo ""

echo "📖 Example/Test Files:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
find . -name "*example*" -o -name "*Example*" -o -path "*/test/*" -name "*.hs" | head -10
echo ""

echo "📄 README files:"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
find . -name "README*" -type f
echo ""

if [ -f "README.md" ]; then
    echo "📖 README.md excerpt:"
    echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
    head -50 README.md
    echo ""
fi

echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
echo "✅ Investigation complete!"
echo ""
echo "Next steps:"
echo "  1. Check the cabal files for package names"
echo "  2. Look at exposed modules to find API"
echo "  3. Check example/test files for usage patterns"
echo "  4. Update flake.nix with correct path"
echo "  5. Update ConformanceGen.hs with correct imports"
echo ""
echo "Results saved in: $CLONE_DIR"
echo "━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━"
