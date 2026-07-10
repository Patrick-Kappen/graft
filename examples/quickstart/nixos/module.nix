# Import this alongside inputs.graft.nixosModules.graft from an existing
# NixOS flake configuration. The exported module supplies Graft by default.
{
  # Graft does not enable or configure the container host for you.
  virtualisation.podman.enable = true;

  services.graft = {
    enable = true;
    configRoot = ./containers;
  };
}
