#!/bin/bash
# Quick start script for Cardano LSM development

set -e

echo "=== Cardano LSM Tree - Development Setup ==="
echo ""

# Check if Rust is installed
if ! command -v cargo &> /dev/null; then
    echo "❌ Rust is not installed!"
    echo "Install it with: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
fi

echo "✅ Rust found: $(rustc --version)"
echo ""

# Build the project
echo "📦 Building project..."
cargo build
echo ""

# Run a single test to verify setup
echo "🧪 Running single test to verify setup..."
cargo test --test test_basic_operations test_empty_tree_lookup -- --nocapture
echo ""

# Show test summary
echo "📊 Test Summary:"
echo "  Total test files: 7"
echo "  Total tests: ~127"
echo ""

# Run all basic operations tests
echo "🚀 Running all basic operation tests..."
cargo test --test test_basic_operations
echo ""

echo "=========================================="
echo "Setup complete!"
echo ""
echo "Next steps:"
echo "  1. cargo test                          # Run all tests"
echo "  2. cargo test --test test_basic_operations  # Run specific file"
echo "  3. cargo test test_single_insert       # Run specific test"
echo "  4. cargo test -- --nocapture           # Show println output"
echo ""
echo "Happy coding! 🦀"
