{
  perSystem = {
    config,
    pkgs,
    ...
  }: {
    devShells.default = with pkgs;
      mkShell {
        packages = [
          cargo
          cmake
          rustc
          pkg-config
          openssl
          zlib
          rust-analyzer
          rustfmt
          libclang
          clippy
          clang-tools
          config.treefmt.build.wrapper
        ];
      };
  };
}
