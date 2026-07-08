{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.graft;

  materialised = import ./lib/materialise-containers.nix {
    inherit lib pkgs cfg;
    target = "system";
    optionName = "services.graft";
  };

in
{
  options.services.graft = {
    enable = lib.mkEnableOption "Graft — TOML-driven Podman Quadlet containers";

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
        message = "services.graft.package must be set when services.graft.configRoot is set.";
      }
    ];

    environment.etc = lib.mapAttrs' (
      name: _:
      lib.nameValuePair "containers/systemd/${lib.removeSuffix ".toml" name}.container" {
        text = materialised.quadletFiles.${name};
      }
    ) materialised.containers;

  };
}
