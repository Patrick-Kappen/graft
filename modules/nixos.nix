{ config, lib, pkgs, ... }:

let
  cfg = config.services.graft;
in {
  options.services.graft = {
    enable = lib.mkEnableOption "Graft — TOML-driven Podman Quadlet containers";

    configRoot = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Directory containing .toml container definitions.";
    };
  };

  config = lib.mkIf cfg.enable {
    # Ensure Podman is enabled
    virtualisation.podman.enable = true;

    # TODO: read TOML files, build buildEnv, render Quadlet files
  };
}
