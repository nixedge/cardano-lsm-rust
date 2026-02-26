{inputs, ...}: {
  perSystem = {
    config,
    pkgs,
    ...
  }: {
    devShells.default = with pkgs;
      mkShell {
        packages = [
          # Rust toolchain
          cargo
          cmake
          rustc
          pkg-config
          openssl
          zlib
          rust-analyzer
          rustfmt
          clippy
          
          # Haskell toolchain
          ghc
          cabal-install
          haskell-language-server
          
          # LSM-tree and conformance generator from our packages
          config.packages.lsm-tree-haskell
          config.packages.conformance-generator
          
          # Task runner
          just
          
          # Utilities
          jq
          fd
          ripgrep
          
          # Tree formatter
          config.treefmt.build.wrapper
        ];
        
        shellHook = ''
          echo "🦀 Cardano LSM Development Environment"
          echo ""
          echo "Rust: $(rustc --version)"
          echo "Cargo: $(cargo --version)"
          echo "GHC: $(ghc --version)"
          echo ""
          echo "📚 Available:"
          echo "  ✅ Rust LSM implementation"
          echo "  ✅ Haskell lsm-tree (from Nix input)"
          echo "  ✅ Conformance generator"
          echo ""
          echo "Commands:"
          echo "  just --list              # Show all commands"
          echo "  just test                # Run Rust tests"
          echo "  just gen-conformance 10  # Generate conformance tests"
          echo "  just test-conformance    # Run conformance tests"
        '';
      };
  };
}
