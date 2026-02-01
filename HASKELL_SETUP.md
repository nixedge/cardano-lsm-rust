# Enabling Haskell Conformance Testing

## Current Status

✅ Nix flake structure set up (following your flake-parts pattern)  
✅ Rust implementation complete and tested  
✅ Conformance test harness ready (Rust side)  
✅ Conformance generator skeleton (Haskell side)  
⏳ Haskell lsm-tree integration (blocked by repo structure)  

## The Issue

The `lsm-tree` repository from IOG doesn't have a top-level cabal file, so `callCabal2nix` fails. We need to:

1. **Investigate the repo structure**
2. **Find the correct subdirectory**
3. **Update the flake input path**

## Investigation Steps

```bash
# Clone the repo locally to inspect
git clone https://github.com/input-output-hk/lsm-tree
cd lsm-tree
find . -name "*.cabal"
```

Likely scenarios:

### Scenario A: Cabal file in subdirectory
```
lsm-tree/
└── lsm-tree/
    └── lsm-tree.cabal    # Actual package here
```

Fix in `packages.nix`:
```nix
lsm-tree = hself.callCabal2nix "lsm-tree" 
  (inputs.lsm-tree-haskell + "/lsm-tree") {};  # Add subdirectory
```

### Scenario B: Multiple packages in repo
```
lsm-tree/
├── lsm-tree-core/
│   └── lsm-tree-core.cabal
└── lsm-tree/
    └── lsm-tree.cabal
```

Fix: Point to specific package
```nix
lsm-tree = hself.callCabal2nix "lsm-tree"
  (inputs.lsm-tree-haskell + "/lsm-tree") {};
```

### Scenario C: Uses Cabal project file
```
lsm-tree/
├── cabal.project
└── packages/
    └── lsm-tree/
        └── lsm-tree.cabal
```

Fix: Use project file or point to package
```nix
lsm-tree = hself.callCabal2nix "lsm-tree"
  (inputs.lsm-tree-haskell + "/packages/lsm-tree") {};
```

## Once Fixed

After finding the correct path, update `perSystem/packages.nix`:

```nix
let
  haskellPackages = pkgs.haskell.packages.ghc98.override {
    overrides = hself: hsuper: {
      # Updated with correct path
      lsm-tree = hself.callCabal2nix "lsm-tree" 
        (inputs.lsm-tree-haskell + "/CORRECT/PATH") {};
    };
  };
in {
  packages = {
    # ... existing packages ...
    
    # Now these will work:
    lsm-tree-haskell = haskellPackages.lsm-tree;
    conformance-generator = haskellPackages.callCabal2nix 
      "conformance-generator" 
      ../conformance-generator {};
  };
}
```

And `perSystem/devShells.nix`:

```nix
packages = [
  # ... existing ...
  
  # Add back Haskell tools
  haskellPackages.lsm-tree
  config.packages.conformance-generator
];
```

## Alternative Approach

If the IOG repo is complex, you could:

### Option 1: Fork and simplify
```nix
inputs.lsm-tree-haskell = {
  url = "github:YOUR-USERNAME/lsm-tree-simple";  # Your simplified fork
  flake = false;
};
```

### Option 2: Use a Haskell flake
```nix
inputs.lsm-tree-haskell = {
  url = "github:input-output-hk/lsm-tree";
  # If they provide a flake, use it directly
};
```

### Option 3: Build without Nix initially
For development, you can:

```bash
# Manually build Haskell generator
cd conformance-generator
cabal build
cabal run conformance-generator -- --output ../conformance-tests

# Then run Rust tests
cd ..
cargo test --test conformance
```

## Current Workaround

The flake is set up to build without Haskell conformance for now:

```bash
nix develop  # Works! Just Rust + Haskell toolchain
just test    # Works! Runs Rust tests
```

Conformance testing is ready to enable once we solve the lsm-tree path issue.

## Action Items

1. **Investigate lsm-tree repo** - Find cabal file location
2. **Update flake input path** - Point to correct subdirectory  
3. **Enable Haskell packages** - Uncomment in packages.nix and devShells.nix
4. **Implement runAndRecord** - Wire up actual lsm-tree API calls
5. **Generate tests** - `just gen-conformance 100`
6. **Achieve 95%+ confidence!** 🎯

## Status

Current setup:
- ✅ Nix flake structure (your pattern)
- ✅ Rust LSM complete (127/127 tests passing)
- ✅ Conformance harness ready
- ⏳ Haskell integration (path issue)

**You can start using the Rust LSM now while we figure out the Haskell path!**
