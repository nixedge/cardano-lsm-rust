# Conformance Testing with Haskell lsm-tree

## Overview

This implements property-based conformance testing between the Rust port and Haskell reference implementation, similar to how Cardano validates the Haskell ledger against Agda specs.

## Architecture

```
┌──────────────────────────────────────────────────────────┐
│           Nix Flake (hermetic build)                     │
├──────────────────────────────────────────────────────────┤
│                                                          │
│  Input: lsm-tree-haskell (github:input-output-hk)       │
│           ↓                                              │
│  Build: Haskell lsm-tree library                        │
│  Build: conformance-generator (Haskell)                 │
│  Build: cardano-lsm (Rust)                              │
│                                                          │
└──────────────────────────────────────────────────────────┘
                             ↓
┌──────────────────────────────────────────────────────────┐
│          Conformance Test Generation                     │
│                                                          │
│  1. conformance-generator                                │
│     • Uses QuickCheck to generate random ops             │
│     • Seeds: 1, 2, 3, ..., N                            │
│     • Runs ops against Haskell lsm-tree                 │
│     • Outputs: test_N.json + test_N.expected.json       │
│                                                          │
└──────────────────────────────────────────────────────────┘
                             ↓
┌──────────────────────────────────────────────────────────┐
│           Conformance Test Execution                     │
│                                                          │
│  For each test_N.json:                                   │
│    1. Load operation sequence                            │
│    2. Run against Rust cardano-lsm                       │
│    3. Compare with test_N.expected.json                  │
│    4. Assert equality                                    │
│                                                          │
│  Result: Rust matches Haskell! ✅                        │
└──────────────────────────────────────────────────────────┘
```

## Usage

### With Nix Flake (Recommended)

```bash
# Enter development shell
nix develop

# Generate conformance tests (Haskell)
just gen-conformance

# Run conformance tests (Rust)
just test-conformance

# Or do both
just conformance
```

### With Just Commands

```bash
# List all commands
just --list

# Generate 100 test cases
just gen-conformance 100

# Run conformance tests
just test-conformance

# Full workflow
just conformance 100

# Run with specific seed
just test-seed 42
```

### Manual

```bash
# 1. Generate test cases
conformance-generator \
  --output conformance-tests \
  --num-tests 100 \
  --max-ops 1000 \
  --seed-start 1

# 2. Run Rust conformance tests
cargo test --test conformance

# 3. Check results
ls conformance-tests/
```

## Test Case Format

### test_42.json
```json
{
  "version": "1.0",
  "seed": 42,
  "config": {
    "memtable_size": 4096,
    "bloom_bits_per_key": 10,
    "level0_compaction_trigger": 4
  },
  "operations": [
    {
      "type": "Insert",
      "key": "a2V5MQ==",
      "value": "dmFsdWUx"
    },
    {
      "type": "Get",
      "key": "a2V5MQ=="
    },
    {
      "type": "Snapshot",
      "id": "snap_0"
    },
    {
      "type": "Delete",
      "key": "a2V5MQ=="
    },
    {
      "type": "Rollback",
      "snapshot_id": "snap_0"
    },
    {
      "type": "Compact"
    }
  ]
}
```

### test_42.expected.json
```json
{
  "results": [
    {"OkUnit": null},
    {"Ok": "dmFsdWUx"},
    {"OkUnit": null},
    {"OkUnit": null},
    {"OkUnit": null},
    {"OkUnit": null}
  ]
}
```

## How It Works

### 1. Test Generation (Haskell)

```haskell
-- Generate random operation sequence
generateTestCase seed numOps = do
  setStdGen (mkStdGen seed)  -- Deterministic!
  ops <- generate $ genOperationSequence numOps
  return TestCase { seed, operations = ops, ... }

-- Run against Haskell lsm-tree
runAndRecord testCase = do
  tree <- LSM.new config
  results <- forM operations $ \op -> do
    case op of
      Insert k v -> LSM.insert tree k v >> return OkUnit
      Get k -> LSM.lookup tree k >>= \v -> return (Ok v)
      Delete k -> LSM.delete tree k >> return OkUnit
      -- ... etc
  return $ TestResults results
```

### 2. Test Execution (Rust)

```rust
// Load test case JSON
let test_case: TestCase = serde_json::from_str(&json)?;

// Run against Rust LSM
let mut tree = LsmTree::open(path, config)?;
for op in test_case.operations {
    match op {
        Insert { key, value } => tree.insert(&key, &value)?,
        Get { key } => tree.get(&key)?,
        // ... etc
    }
}

// Compare with expected results
assert_eq!(rust_results, haskell_results);
```

## Benefits

### vs Manual Unit Tests

| Manual Tests | Conformance Tests |
|--------------|-------------------|
| ~127 tests | 100-1000+ tests |
| Fixed scenarios | Random scenarios |
| Human-designed | Property-generated |
| Single impl | Two impls compared |

### What This Catches

✅ **Semantic Bugs** - Different behavior between implementations  
✅ **Edge Cases** - Scenarios you didn't think of  
✅ **Compaction Differences** - Subtle LSM semantics  
✅ **Tombstone Handling** - Multi-level delete propagation  
✅ **Snapshot Isolation** - MVCC semantics  
✅ **Ordering** - Sort order differences  

## Development Workflow

### Initial Setup
```bash
# Clone and enter shell
nix develop

# Generate initial test suite
just gen-conformance 10

# Run and fix issues
just test-conformance
```

### Continuous Development
```bash
# Watch for changes and rerun tests
just watch

# After Rust changes
just test-conformance

# After fixing a bug
just conformance 100  # Regenerate and test
```

### Before Release
```bash
# Generate large test suite
just gen-conformance 1000

# Run full conformance
just test-conformance

# All should pass!
```

## Nix Integration

### Flake Structure
```
flake.nix
├── inputs
│   ├── nixpkgs (25.11)
│   ├── naersk (Rust builder)
│   └── lsm-tree-haskell (reference impl)
│
├── perSystem
│   ├── packages.nix
│   │   ├── cardano-lsm (Rust)
│   │   ├── lsm-tree-haskell (Haskell)
│   │   └── conformance-generator (Haskell)
│   │
│   └── devShells.nix
│       └── Both Rust + Haskell toolchains
│
└── formatter.nix
```

### Building Everything
```bash
# Build Rust LSM
nix build

# Build conformance generator
nix build .#conformance-generator

# Build Haskell lsm-tree
nix build .#lsm-tree-haskell
```

## Confidence Levels

| Scenario | Confidence |
|----------|------------|
| After 127 unit tests | 75-80% |
| After 100 conformance tests | 90% |
| After 1000 conformance tests | 95%+ |
| After testnet usage | 98% |
| After mainnet usage | 99%+ |

## Timeline

**Week 1**: Set up infrastructure
- ✅ Nix flake with both implementations
- ✅ JSON schema defined
- ✅ Rust test harness complete
- ⏳ Haskell generator (needs lsm-tree integration)

**Week 2**: Generate and test
- Run conformance suite
- Fix any discrepancies
- Iterate until 100% pass

**Week 3**: Scale up
- Generate 1000+ tests
- Various operation mixes
- Edge case focus

## Current Status

✅ Nix flake structure
✅ Rust conformance test harness  
✅ Manual conformance tests (3 tests)
⏳ Haskell generator (needs lsm-tree API integration)
⏳ Full test suite generation

## Next Steps

1. **Implement runAndRecord in Haskell**
   - Integrate with actual lsm-tree API
   - Handle all operation types
   - Capture results

2. **Generate initial test suite**
   - Start with 10 simple tests
   - Verify Rust passes all
   - Incrementally add complexity

3. **Scale up**
   - 100 tests
   - 1000 tests
   - Continuous conformance in CI

## Files

```
cardano-lsm-rust/
├── flake.nix                  # Main flake (your pattern)
├── flake/lib.nix              # Recursive imports helper
├── perSystem/
│   ├── packages.nix           # Rust + Haskell builds
│   ├── devShells.nix          # Dev environment
│   └── formatter.nix          # Code formatting
├── justfile                   # Task runner commands
├── tests/conformance.rs       # Rust test harness
└── conformance-generator/     # Haskell test generator
    ├── conformance-generator.cabal
    ├── src/ConformanceGen.hs
    └── app/Main.hs
```

This is the Cardano way! 🎯
