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
        pkgsRiscV = nixpkgs.legacyPackages.${system}.pkgsCross.riscv64;

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

        eapp = pkgsRiscV.stdenv.mkDerivation {
          name = "bqti-eapp";
          src = ./enclave/eapp;
          buildPhase = ''
            $CC bqti.c -I ${./keystone-sdk/include/app} -o bqti
          '';
          installPhase = "mkdir -p $out/bin && cp bqti $out/bin/";
        };

      in
      {
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            transmission_4
          ];

          shellHook = "";
        };

        packages = {
          default = bqti;
          inherit eapp;
        };

        apps = {

          default = flake-utils.lib.mkApp {
            drv = bqti;
            name = "bqti";
          };

          gen-tee = {
            type = "app";
            program = toString (pkgs.writeShellScript "deploy" ''
                cd bqti-enclave
                ./build.sh
                cd build && make bqti-package
                cd ..
                scp -O build/bqti.ke keystone-vm:/root/bqti.ke
                echo "deployed"
                '');
          };

          gen-manifest = {
            type = "app";
            program = toString (pkgs.writeShellScript "gen-manifest" ''
                set -e

                if [ -z "$1" ]; then
                  echo "usage: nix run .#gen-manifest -- <enclave-hash>"
                  exit 1
                fi

                CA_KEY="''${BQTI_CA_KEY:-$HOME/.bqti-pki/main_ca.pem}"

                if [ ! -f "$CA_KEY" ]; then
                  echo "error: CA private key not found at $CA_KEY"
                  echo "set BQTI_CA_KEY=/path/to/main_ca.pem or place it at ~/.bqti-pki/main_ca.pem"
                  exit 1
                fi

                ENCLAVE_HASH="$1"

                VERSION=$(${pkgs.cargo}/bin/cargo metadata --no-deps --format-version 1 | ${pkgs.jq}/bin/jq -r '.packages[] | select(.name == "bqti") | .version')

                ${pkgs.coreutils}/bin/cat > docs/manifest.json << EOF
                {
                  "version": "$VERSION",
                  "enclave_hash": "$ENCLAVE_HASH"
                }
                EOF

                ${pkgs.openssl}/bin/openssl pkeyutl -sign \
                  -inkey $CA_KEY \
                  -in docs/manifest.json \
                  -out docs/manifest.sig

                echo "done."
            '');
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
