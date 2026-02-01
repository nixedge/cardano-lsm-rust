# LSM-Tree API Integration Guide

## Objective

Wire up the conformance generator to use the actual Haskell lsm-tree implementation from Input Output Global (IOG).

## Repository Structure Investigation

The lsm-tree repository likely has one of these structures:

### Option 1: Multi-package Cabal Project
```
lsm-tree/
├── cabal.project
├── lsm-tree/
│   ├── lsm-tree.cabal
│   └── src/Database/LSMTree/...
├── lsm-tree-extras/
│   └── lsm-tree-extras.cabal
└── test/
```

**Nix fix**: Point to subdirectory
```nix
lsm-tree = hself.callCabal2nix "lsm-tree" 
  "${inputs.lsm-tree-haskell}/lsm-tree" {};
```

### Option 2: Single Package
```
lsm-tree/
├── lsm-tree.cabal
└── src/Database/LSMTree/...
```

**Nix fix**: Use as-is
```nix
lsm-tree = hself.callCabal2nix "lsm-tree" inputs.lsm-tree-haskell {};
```

## Expected LSM-Tree API

Based on common LSM tree implementations and Cardano patterns, the API likely looks like:

### Module Structure
```haskell
Database.LSMTree
  -- Main module, re-exports common functions

Database.LSMTree.Normal
  -- Normal (non-monoidal) LSM tree operations

Database.LSMTree.Monoidal  
  -- Monoidal value operations

Database.LSMTree.Common
  -- Shared types and utilities
```

### Core API Functions

```haskell
-- Session management (resource management)
type Session = ...

withSession 
  :: SessionConfig 
  -> FilePath           -- Database directory
  -> (Session -> IO a) 
  -> IO a

data SessionConfig = SessionConfig
  { confMergePolicy :: MergePolicy
  , confBloomFilterAlloc :: Int
  , ...
  }

defaultSessionConfig :: SessionConfig

-- Table operations
type Table k v = ...

withTable
  :: Session
  -> TableConfig
  -> (Table k v -> IO a)
  -> IO a

data TableConfig = TableConfig { ... }

defaultTableConfig :: TableConfig

-- CRUD operations
insert :: Table k v -> k -> v -> IO ()
lookup :: Table k v -> k -> IO (Maybe v)
delete :: Table k v -> k -> IO ()

-- Range queries (might have different signatures)
rangeLookup 
  :: Table k v 
  -> Maybe k      -- Lower bound
  -> Maybe k      -- Upper bound  
  -> IO [(k, v)]

-- OR cursor-based
newCursor :: Table k v -> k -> IO (Cursor k v)
readCursor :: Cursor k v -> IO (Maybe (k, v))

-- Compaction (might be automatic or manual)
compact :: Table k v -> IO ()
-- OR
flush :: Table k v -> IO ()

-- Snapshots (if supported - might not be in public API)
snapshot :: Table k v -> IO Snapshot
restore :: Table k v -> Snapshot -> IO ()
```

## Integration Steps

### Step 1: Find the API

```bash
# Clone the repo
git clone https://github.com/input-output-hk/lsm-tree
cd lsm-tree

# Find exposed modules
grep -r "exposed-modules" --include="*.cabal"

# Check for examples or tests
find . -name "*example*" -o -name "*Example*"
find . -type f -name "*.hs" -path "*/test/*" | head -5

# Read the main module
cat src/Database/LSMTree.hs
# Or wherever the main module is
```

### Step 2: Update Imports in ConformanceGen.hs

Replace line 32-34 with actual imports:

```haskell
-- Current placeholder:
import qualified Database.LSMTree as LSM
import qualified Database.LSMTree.Normal as LSM

-- Update to actual modules, e.g.:
import qualified Database.LSMTree as LSM
import qualified Database.LSMTree.Normal as Normal
import qualified Database.LSMTree.Common as Common
```

### Step 3: Update runWithLSMTree

Replace lines ~200-215:

```haskell
runWithLSMTree :: TestCase -> IO TestResults
runWithLSMTree TestCase{..} = 
  withSystemTempDirectory "lsm-conformance" $ \tmpDir -> do
    -- Use actual lsm-tree API
    LSM.withSession sessionConfig tmpDir $ \session ->
      LSM.withTable session tableConfig $ \table -> do
        results <- executeOperations table operations
        return $ TestResults { results }
  where
    sessionConfig = LSM.defaultSessionConfig
      { LSM.confBloomFilterAlloc = bloom_bits_per_key config
      -- Add other config fields
      }
    
    tableConfig = LSM.defaultTableConfig
      -- Add table-specific config
```

### Step 4: Update executeOperations

Replace the operation execution functions (lines ~220-270):

```haskell
doExecute table _ (Insert k v) = do
  LSM.inserts table [(k, v)]  -- Batch insert
  -- OR: LSM.insert table k v  -- Single insert
  return OkUnit

doExecute table _ (Get k) = do
  maybeValue <- LSM.lookups table [k]  -- Batch lookup
  -- OR: maybeValue <- LSM.lookup table k
  case maybeValue of
    [Just v] -> return $ Ok (Just (encodeB64 v))
    _ -> return $ Ok Nothing

doExecute table _ (Delete k) = do
  LSM.deletes table [k]  -- Batch delete
  -- OR: LSM.delete table k
  return OkUnit

doExecute table _ (Range from to) = do
  -- Check actual range API
  entries <- LSM.rangeLookup table (Just from) (Just to)
  let pairs = [(encodeB64 k, encodeB64 v) | (k, v) <- entries]
  return $ OkRange pairs

doExecute table _ Compact = do
  -- Check if manual compaction is available
  LSM.compact table
  -- OR just return OkUnit if automatic
  return OkUnit
```

### Step 5: Handle Snapshots

If lsm-tree supports snapshots:

```haskell
-- Add to state
type SnapshotState = Map String SnapshotHandle

doExecute table snapshots (Snapshot sid) = do
  snap <- LSM.snapshot table
  -- Store snap in snapshots map
  return OkUnit

doExecute table snapshots (Rollback sid) = do
  case Map.lookup sid snapshots of
    Just snap -> do
      LSM.restore table snap
      return OkUnit
    Nothing -> 
      return $ Err "Snapshot not found"
```

If snapshots aren't supported, keep the reference model behavior for those operations.

## Testing Strategy

### Phase 1: Basic Operations (Current)
- [x] Generate test cases
- [x] Use reference model
- [x] Test Rust harness works
- [ ] Wire up lsm-tree API

### Phase 2: Full Integration
- [ ] All CRUD operations use real lsm-tree
- [ ] Range queries work
- [ ] Compaction if available
- [ ] Snapshots if available

### Phase 3: Validation
- [ ] Generate 100 test cases
- [ ] Rust passes all conformance tests
- [ ] 95%+ confidence achieved!

## Fallback Strategy

If lsm-tree snapshot/rollback APIs don't exist or are too complex:

1. **Skip those operations** - Only test CRUD + Range + Compact
2. **Model state separately** - Keep a Map for snapshots
3. **Hybrid approach** - Real LSM for CRUD, model for snapshots

The reference model ensures we can ALWAYS generate useful tests.

## Error Handling

The generator handles errors gracefully:

```haskell
result <- try @SomeException $ runWithLSMTree testCase

case result of
  Right testResults -> return testResults
  Left err -> do
    putStrLn $ "⚠️  lsm-tree error: " ++ show err
    runAndRecordReference testCase  -- Fallback
```

This means even if lsm-tree integration fails, we still generate tests!

## Success Criteria

✅ Generator compiles
✅ Generates valid JSON
✅ JSON matches schema
✅ Rust harness can load tests
✅ Tests run against Rust implementation
🎯 Results match between Rust and Haskell

## Questions to Answer

When investigating lsm-tree:

1. What are the exposed modules?
2. How do you create a session/table?
3. What's the signature of insert/lookup/delete?
4. Is there a range query function?
5. Is there a compact function?
6. Are snapshots supported?
7. What's the error handling approach?

Document answers in: `lsm-tree-API-NOTES.md`
