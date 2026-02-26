# Justfile for Cardano LSM development

# List available commands
default:
    @just --list

# Run all Rust tests
test:
    cargo test

# Run specific test suite
test-suite suite:
    cargo test --test {{suite}}

# Run tests with output
test-verbose:
    cargo test -- --nocapture

# Build the Rust library
build:
    cargo build

# Build optimized
build-release:
    cargo build --release

# Run clippy linter
lint:
    cargo clippy -- -D warnings

# Format all code (Rust + Nix)
fmt:
    cargo fmt
    nix fmt

# Clean build artifacts
clean:
    cargo clean
    rm -rf conformance-tests target

# Run all checks (lint + test)
check: lint test

# Watch tests (requires cargo-watch)
watch:
    cargo watch -x test

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Conformance Testing
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Investigate lsm-tree repository structure
investigate-lsm:
    ./scripts/investigate-lsm-tree.sh

# Build Haskell conformance generator
build-conformance:
    cd conformance-generator && cabal build

# Generate conformance test cases
gen-conformance num="100" max-ops="1000":
    @echo "Generating {{num}} conformance test cases..."
    @mkdir -p conformance-tests
    cd conformance-generator && cabal run conformance-generator -- \
        --output ../conformance-tests \
        --num-tests {{num}} \
        --max-ops {{max-ops}} \
        --seed-start 1 \
        --verbose

# Run conformance tests (Rust against generated cases)
test-conformance:
    @echo "Running Rust conformance tests..."
    cargo test --test conformance -- --nocapture

# Full conformance workflow: build generator + generate + test
conformance num="100": build-conformance (gen-conformance num) test-conformance

# Run conformance with specific seed
conformance-seed seed:
    @echo "Testing with seed {{seed}}..."
    cd conformance-generator && cabal run conformance-generator -- \
        --output ../conformance-tests \
        --num-tests 1 \
        --max-ops 100 \
        --seed-start {{seed}}
    cargo test --test conformance -- --nocapture

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Nix Commands
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Build with Nix
nix-build:
    nix build

# Check Nix flake
nix-check:
    nix flake check

# Update Nix flake inputs
nix-update:
    nix flake update

# Show Nix flake outputs
nix-show:
    nix flake show

# Build conformance generator with Nix
nix-build-conformance:
    nix build .#conformance-generator

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Development
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Run specific test with seed
test-seed seed:
    PROPTEST_SEED={{seed}} cargo test

# Profile a specific test
profile test-name:
    cargo test {{test-name}} --release -- --nocapture

# Generate test coverage (requires cargo-tarpaulin)
coverage:
    cargo tarpaulin --out Html --output-dir target/coverage
    @echo "Coverage report: target/coverage/index.html"

# Run benchmarks (when implemented)
bench:
    cargo bench

# Check documentation
doc:
    cargo doc --no-deps --open

# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
# Quick Workflows
# ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

# Full test suite
test-all: lint test test-conformance

# Quick development cycle
dev: fmt check

# Pre-commit checks
pre-commit: fmt lint test

# Prepare for release
release: clean fmt lint test conformance nix-check
