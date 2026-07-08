{ config, lib, pkgs, ... }:

let
  cfg = config.programs.graft;

  materialised = import ./lib/materialise-containers.nix {
    inherit lib pkgs cfg;
    target = "user";
    optionName = "programs.graft";
  };

in
{
  options.programs.graft = {
    enable = lib.mkEnableOption "Graft — TOML-driven Podman Quadlet containers (user)";

    package = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default = null;
      description = "Graft package providing the graft CLI and graft-pause binary.";
    };

    configRoot = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Directory containing .toml container definitions.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = cfg.configRoot == null || cfg.package != null;
        message = "programs.graft.package must be set when programs.graft.configRoot is set.";
      }
    ];

    xdg.configFile = lib.mapAttrs'
      (name: _:
        lib.nameValuePair
          "containers/systemd/${lib.removeSuffix ".toml" name}.container"
          { text = materialised.quadletFiles.${name}; }
      )
      materialised.containers;

  };
}
