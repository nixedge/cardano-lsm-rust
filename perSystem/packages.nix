{inputs, ...}: {
  perSystem = {
    system,
    config,
    lib,
    pkgs,
    ...
  }: let
    # Build conformance generator standalone (without lsm-tree for now)
    # Uses reference model until we integrate haskell.nix properly
    haskellPackages = pkgs.haskellPackages;
    
    conformance-gen = haskellPackages.callCabal2nix "conformance-generator" ../conformance-generator {};
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
      
      # Conformance test generator (uses reference model for now)
      # TODO: Integrate with haskell.nix to use real lsm-tree
      conformance-generator = conformance-gen;
    };
  };
}
