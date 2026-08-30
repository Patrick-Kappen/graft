# Import this alongside inputs.graft.homeManagerModules.graft from an existing
# Home Manager configuration. The exported module supplies Graft by default.
{
  programs.graft = {
    enable = true;
    hostId = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20";
    configRoot = ./containers;
  };
}
