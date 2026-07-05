{
  description = "Graft — NixOS Podman Quadlet containers from TOML";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";
    flake-parts = {
      url = "github:hercules-ci/flake-parts";
      inputs.nixpkgs-lib.follows = "nixpkgs";
    };
  };

  outputs = inputs: inputs.flake-parts.lib.mkFlake { inherit inputs; } {
    systems = [ "x86_64-linux" "aarch64-linux" ];

    flake = {
      nixosModules.graft        = import ./modules/nixos.nix;
      homeManagerModules.graft  = import ./modules/home-manager.nix;
    };
  };
}
