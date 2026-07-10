# Import this module from an existing Home Manager configuration.
# The exported Graft flake module supplies the Graft package by default.
{ inputs, ... }:
{
  imports = [ inputs.graft.homeManagerModules.graft ];

  programs.graft = {
    enable = true;
    configRoot = ./containers;
  };
}
