# Example: using graft in a NixOS flake configuration.
#
# In your flake.nix inputs:
#   graft.url = "github:Patrick-Kappen/graft";
#
# Then import this module (or inline it) in your NixOS configuration.

{ inputs, pkgs, ... }:
{
  imports = [ inputs.graft.nixosModules.graft ];

  services.graft = {
    enable = true;

    # Package providing the graft CLI and graft-pause binary.
    package = inputs.graft.packages.${pkgs.stdenv.hostPlatform.system}.default;

    # Directory containing your .toml container definitions.
    # Place it wherever makes sense in your repo.
    configRoot = ./containers;
  };
}
