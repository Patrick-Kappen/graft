{ config, lib, pkgs, ... }:

let
  cfg = config.programs.graft;
in {
  options.programs.graft = {
    enable = lib.mkEnableOption "Graft — TOML-driven Podman Quadlet containers (user)";

    configRoot = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Directory containing .toml container definitions.";
    };
  };

  config = lib.mkIf cfg.enable {
    # TODO: read TOML files, build buildEnv, render Quadlet files
    # Places rendered Quadlet files in ~/.config/containers/systemd/
  };
}
