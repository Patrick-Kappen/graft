{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.programs.podman-agent-container;
  evaluated = import ../lib/eval-entries.nix {
    inherit lib pkgs;
    inherit (cfg) package configRoot configFiles;
    deployTarget = "user";
    optionPrefix = "programs.podman-agent-container";
  };
in
{
  options.programs.podman-agent-container = {
    enable = lib.mkEnableOption "TOML-driven rootless Podman Quadlet containers";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.system}.podman-agent-container;
      defaultText = lib.literalExpression "self.packages.${pkgs.system}.podman-agent-container";
      description = "podman-agent-container package to use for rendering TOML to Quadlet.";
    };

    configFiles = lib.mkOption {
      type = lib.types.listOf lib.types.path;
      default = [ ];
      description = ''
        Explicit user-authored TOML config files. Each active file must set top-level `name`.
        Explicit files are active when they are not no-op. Prefer `configRoot` for the
        directory-discovery workflow.
      '';
    };

    configRoot = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = ''
        Directory containing user-authored TOML config files. Files are discovered
        recursively. A discovered TOML file becomes a Home Manager user Quadlet only
        when it is not no-op and sets `[deploy] enable = true` with `target = "user"`.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = evaluated.activeNames == evaluated.uniqueActiveNames;
        message = "programs.podman-agent-container: active TOML config names must be unique.";
      }
    ]
    ++ map (entry: {
      assertion = entry.unknownPackageNames == [ ];
      message = "programs.podman-agent-container: unknown package in ${toString entry.configFile} config.runtime.packages: ${builtins.concatStringsSep ", " entry.unknownPackageNames}";
    }) evaluated.activeEntries;

    home.packages = [ cfg.package ];

    xdg.configFile = lib.listToAttrs (
      map (entry: {
        name = "containers/systemd/${entry.name}.container";
        value.source = entry.renderedQuadlet;
      }) evaluated.activeEntries
    );
  };
}
