{inputs, ...}: {
  perSystem = {
    system,
    config,
    lib,
    pkgs,
    ...
  }: {
    packages = {
      example = let
        naersk-lib = inputs.naersk.lib.${system};
      in
        naersk-lib.buildPackage rec {
          pname = "example";

          src = with lib.fileset;
            toSource {
              root = ./..;
              fileset = unions [
                ../Cargo.lock
                ../Cargo.toml
                ../src
              ];
            };

          buildInputs = with pkgs; [
            pkg-config
          ];

          meta = {
            mainProgram = pname;
            maintainers = with lib.maintainers; [
              disassembler
            ];
            license = with lib.licenses; [
              asl20
            ];
          };
        };
    };
  };
}
