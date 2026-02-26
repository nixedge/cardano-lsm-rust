{inputs, ...}: {
  perSystem = {
    system,
    config,
    lib,
    pkgs,
    ...
  }: let
    # Use haskell.nix for IOG Haskell projects
    haskellNixPkgs = import inputs.haskellNix {
      inherit system;
      inherit (pkgs) config;
      overlays = [
        # CHaP overlay for Cardano packages
        (_: _: {
          CHaP = inputs.CHaP;
        })
      ];
    };
    
    # Build lsm-tree and conformance-generator with haskell.nix
    # This handles the broken dependencies properly
    project = haskellNixPkgs.haskell-nix.cabalProject' {
      src = inputs.lsm-tree-haskell;
      compiler-nix-name = "ghc98";
      
      # Add CHaP as a package source
      inputMap = {
        "https://chap.intersectmbo.org/" = inputs.CHaP;
      };
      
      modules = [{
        # Allow building packages marked as broken
        packages = {
          quickcheck-state-machine.doHaddock = false;
          quickcheck-state-machine.flags = {};
        };
      }];
    };
    
    # Build our conformance generator separately
    conformanceProject = haskellNixPkgs.haskell-nix.cabalProject' {
      src = ../conformance-generator;
      compiler-nix-name = "ghc98";
      
      inputMap = {
        "https://chap.intersectmbo.org/" = inputs.CHaP;
      };
      
      # Make lsm-tree available from the other project
      modules = [{
        packages = {
          conformance-generator = {
            components.exes.conformance-generator = {
              # Link against lsm-tree from the project
            };
          };
        };
      }];
    };
    
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
              ];
            };

          nativeBuildInputs = with pkgs; [
            pkg-config
          ];
          
          doCheck = true;

          meta = {
            description = "Pure Rust port of Cardano's LSM tree for blockchain indexing";
            license = lib.licenses.asl20;
          };
        };
      
      # Haskell lsm-tree library (built with haskell.nix)
      lsm-tree-haskell = project.hsPkgs.lsm-tree.components.library;
      
      # Conformance test generator
      conformance-generator = conformanceProject.hsPkgs.conformance-generator.components.exes.conformance-generator;
    };
  };
}
