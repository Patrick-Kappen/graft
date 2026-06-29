{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.graft;
  tomlFormat = pkgs.formats.toml { };
  evaluated = import ../lib/eval-entries.nix {
    inherit lib pkgs;
    inherit (cfg) package configRoot configFiles;
    nixContainers = cfg.containers;
    deployTarget = "system";
    optionPrefix = "services.graft";
  };
in
{
  options.services.graft = {
    enable = lib.mkEnableOption "TOML-driven Podman Quadlet containers";

    package = lib.mkOption {
      type = lib.types.package;
      default = self.packages.${pkgs.system}.graft;
      defaultText = lib.literalExpression "self.packages.${pkgs.system}.graft";
      description = "graft package to use for rendering TOML to Quadlet.";
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
        recursively. A discovered TOML file becomes a NixOS-managed container only
        when it is not no-op and sets `[deploy] enable = true` with `target = "system"`
        or no target.
      '';
    };

    containers = lib.mkOption {
      type = lib.types.attrsOf tomlFormat.type;
      default = { };
      example = lib.literalExpression ''
        {
          web.config.runtime = {
            mode = "rootfs-store";
            packages = [ "hello" ];
            command = [ "/bin/hello" ];
          };
        }
      '';
      description = ''
        Containers authored directly in Nix instead of TOML files. Each attribute
        name is the container name (used unless the value sets its own `name`).
        Values mirror the TOML schema (`version`, `parents`, `children`, `deploy`,
        `validation`, `config`) and are serialized to TOML, then flow through the
        same resolver and renderer as file-based configs. Nix-authored containers
        are always active (like `configFiles`); `parents`/`children` graph refs
        resolve against `configRoot`.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = evaluated.activeNames == evaluated.uniqueActiveNames;
        message = "services.graft: active TOML config names must be unique.";
      }
    ]
    ++ map (entry: {
      assertion = !entry.invalidDeployTarget;
      message = "services.graft: invalid deploy target in ${toString entry.configFile}: ${entry.configDeployTarget}";
    }) evaluated.entries
    ++ map (entry: {
      assertion = !entry.invalidRuntimeMode;
      message = "services.graft: unsupported runtime mode in ${toString entry.configFile}; expected rootfs-store";
    }) evaluated.activeEntries
    ++ map (entry: {
      assertion = entry.invalidPackageNames == [ ];
      message = "services.graft: invalid package name in ${toString entry.configFile} config.runtime.packages: ${builtins.concatStringsSep ", " entry.invalidPackageNames}";
    }) evaluated.activeEntries
    ++ map (entry: {
      assertion = entry.unknownPackageNames == [ ];
      message = "services.graft: unknown package in ${toString entry.configFile} config.runtime.packages: ${builtins.concatStringsSep ", " entry.unknownPackageNames}";
    }) evaluated.activeEntries;

    virtualisation.podman.enable = true;

    environment.etc = lib.listToAttrs (
      lib.concatMap (
        entry:
        map (unitName: {
          name = "containers/systemd/${unitName}";
          value.source = "${entry.renderedQuadletDir}/${unitName}";
        }) entry.renderedUnitNames
      ) evaluated.activeEntries
    );
  };
}
