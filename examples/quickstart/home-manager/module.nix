# Import this alongside inputs.graft.homeManagerModules.graft from an existing
# Home Manager configuration. The exported module supplies Graft by default.
{
  programs.graft = {
    enable = true;
    configRoot = ./containers;
  };
}
