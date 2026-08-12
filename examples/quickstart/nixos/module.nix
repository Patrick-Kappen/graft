# Import this alongside inputs.graft.nixosModules.graft from an existing
# NixOS flake configuration. The exported module supplies Graft by default.
{
  # Graft does not enable or configure the container host for you.
  virtualisation.podman.enable = true;

  services.graft = {
    enable = true;
    hostId = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20";
    configRoot = ./containers;
  };
}
