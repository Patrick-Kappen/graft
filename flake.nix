{
  description = "Graft — NixOS Podman Quadlet containers from TOML";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = { nixpkgs, ... }: {
    nixosModules.graft       = import ./modules/nixos.nix;
    homeManagerModules.graft = import ./modules/home-manager.nix;
  };
}
