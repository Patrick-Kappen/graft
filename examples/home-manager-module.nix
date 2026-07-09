# Example: using graft in a Home Manager configuration.
#
# In your flake.nix inputs:
#   graft.url = "github:Patrick-Kappen/graft";
#
# Then import this module (or inline it) in your Home Manager configuration.

{ inputs, pkgs, ... }:
{
  imports = [ inputs.graft.homeManagerModules.graft ];

  programs.graft = {
    enable = true;

    # Package providing the graft CLI and graft-pause binary.
    package = inputs.graft.packages.${pkgs.stdenv.hostPlatform.system}.default;

    # Directory containing your .toml container definitions.
    # Place it wherever makes sense in your repo.
    configRoot = ./containers;

    # Optional additional roots, for shared plus host-specific containers.
    # Duplicate TOML filenames or duplicate container names fail evaluation.
    # configRoots = [
    #   ./containers/common
    #   ./hosts/my-host/containers
    # ];
  };
}
