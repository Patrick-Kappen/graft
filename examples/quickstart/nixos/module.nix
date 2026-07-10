# Import this module from an existing NixOS flake configuration.
# The exported Graft flake module supplies the Graft package by default.
{ inputs, ... }:
{
  imports = [ inputs.graft.nixosModules.graft ];

  # Graft does not enable or configure the container host for you.
  virtualisation.podman.enable = true;

  services.graft = {
    enable = true;
    configRoot = ./containers;
  };
}
