# Example: using graft in a NixOS flake configuration.
#
# In your flake.nix inputs:
#   graft.url = "github:Patrick-Kappen/graft";
#
# Then import this module (or inline it) in your NixOS configuration.

{ inputs, ... }:
{
  imports = [ inputs.graft.nixosModules.graft ];

  services.graft = {
    enable = true;

    # Stable fleet-issued, non-secret canonical UUIDv7 host identity.
    hostId = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20";

    # The exported flake module supplies the Graft package by default.
    # Set package explicitly only to override it.

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
