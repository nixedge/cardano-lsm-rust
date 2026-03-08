{inputs, ...}: {
  perSystem = {
    system,
    config,
    lib,
    pkgs,
    ...
  }: let
    # Build conformance generator with broken packages allowed
    # Some transitive dependencies like quickcheck-state-machine are marked broken in nixpkgs
    haskellPackages = pkgs.haskellPackages.override {
      overrides = self: super: {
        # Allow broken packages needed by lsm-tree dependencies
        # Disable tests for quickcheck-state-machine since test dependencies are not available
        quickcheck-state-machine = pkgs.haskell.lib.dontCheck (pkgs.haskell.lib.unmarkBroken super.quickcheck-state-machine);
        blockio-uring = pkgs.haskell.lib.unmarkBroken super.blockio-uring;

        # Override lsm-tree to disable the extras sublibrary (has broken random package usage)
        # Also disable tests since they depend on extras
        lsm-tree = pkgs.haskell.lib.dontCheck (pkgs.haskell.lib.overrideCabal super.lsm-tree (drv: {
          libraryHaskellDepends = builtins.filter (p: p.pname or "" != "extras") (drv.libraryHaskellDepends or []);
          # Disable building extras sublibrary by marking it as not buildable
          postPatch = (drv.postPatch or "") + ''
            sed -i '/^library extras$/a\  buildable: False' lsm-tree.cabal
          '';
        }));
      };
    };

    # Set up conformance generator with lsm-tree source from flake input
    conformance-gen = haskellPackages.callCabal2nix "conformance-generator" (pkgs.runCommand "conformance-generator-src" {} ''
      mkdir -p $out
      cp -r ${../conformance-generator}/* $out/
      chmod -R +w $out

      # Create lsm-tree parent directory structure expected by cabal.project
      mkdir -p $out/../lsm-tree
      cp -r ${inputs.lsm-tree}/lsm-tree $out/../lsm-tree/
      cp -r ${inputs.lsm-tree}/blockio $out/../lsm-tree/

      # Update cabal.project to use the vendored paths
      cat > $out/cabal.project <<EOF
packages: .
  ../lsm-tree/lsm-tree
  ../lsm-tree/blockio

-- Use same index state as lsm-tree for consistency
index-state: 2025-12-10T00:00:00Z

tests: True
benchmarks: False

-- Use serial HasBlockIO to avoid liburing dependency
package blockio
  flags: +serialblockio

-- Specific overrides for known issues
constraints:
  aeson >= 2.2,
  text >= 2.0,
  bytestring >= 0.12

-- Relax bounds
package conformance-generator
  ghc-options: -Wwarn
EOF
    '') {};
  in {
    packages = {
      default = config.packages.cardano-lsm;
      
      # Rust LSM library
      cardano-lsm = let
        naersk-lib = inputs.naersk.lib.${system};
      in
        naersk-lib.buildPackage {
          pname = "cardano-lsm";
          version = "0.1.0";

          src = with lib.fileset;
            toSource {
              root = ./..;
              fileset = unions [
                ../Cargo.lock
                ../Cargo.toml
                ../src
                ../tests
                ../benches
              ];
            };

          nativeBuildInputs = with pkgs; [
            pkg-config
          ];

          doCheck = true;

          # Skip conformance tests in Nix builds (requires 193MB of generated test data)
          cargoTestOptions = x: x ++ ["--" "--skip" "conformance_tests"];

          meta = {
            description = "Pure Rust port of Cardano's LSM tree for blockchain indexing";
            license = lib.licenses.asl20;
          };
        };
      
      # Conformance test generator (uses reference model for now)
      # TODO: Integrate with haskell.nix to use real lsm-tree
      conformance-generator = conformance-gen;
    };
  };
}
