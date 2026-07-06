{
  description = "Graft — NixOS Podman Quadlet containers from TOML";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = { self, nixpkgs, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      nixosModules.graft = { lib, pkgs, ... }: {
        imports = [ ./modules/nixos.nix ];
        services.graft.package = lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      };
      homeManagerModules.graft = import ./modules/home-manager.nix;

      packages = forAllSystems (system:
        let pkgs = nixpkgs.legacyPackages.${system};
        in {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "graft";
            version = "0.1.0";
            src = ./cli;
            cargoLock.lockFile = ./cli/Cargo.lock;
          };
        }
      );
    };
}
