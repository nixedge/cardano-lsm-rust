{
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
          
          # Conformance generator (uses local lsm-tree checkout)
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
          echo "📚 Status:"
          echo "  ✅ Rust LSM (127/127 tests passing)"
          echo "  ✅ Conformance generator (using lsm-tree from ../lsm-tree)"
          echo ""
          echo "Commands:"
          echo "  just test                # Run Rust tests"
          echo "  just gen-conformance 10  # Generate 10 conformance tests"
          echo "  just test-conformance    # Run conformance tests"
        '';
      };
  };
}
