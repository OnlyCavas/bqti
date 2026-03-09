{
  description = "BTQI - BitTorrent client with QUIC + TEE + I2P";

  inputs = {
    nixpkgs.url = "github:nixos/nixpkgs?ref=nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
      ...
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = nixpkgs.legacyPackages.${system};

        bqti = pkgs.rustPlatform.buildRustPackage {
          pname = "btqi";
          version = "0.1.0";
          src = ./.;

          cargoLock.lockFile = ./Cargo.lock;

          nativeBuildInputs = with pkgs; [
            pkg-config
          ];

          buildInputs = with pkgs; [
            openssl
          ];
        };

      in
      {
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
          ];

          shellHook = "";
        };

        packages.default = bqti;

        apps = {

          default = flake-utils.lib.mkApp {
            drv = bqti;
            name = "bqti";
          };

          dev = {
            type = "app";
            program = toString (
              pkgs.writeShellScript "bqti-dev" ''
                RUST_BACKTRACE=1 ${bqti}/bin/bqti "$@"
              ''
            );
          };
        };

      }
    );
}
