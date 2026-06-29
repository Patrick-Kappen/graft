{ self }:
{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.podman-agent-container;

  listTomlFiles =
    dir:
    lib.concatMap (
      name:
      let
        entryType = (builtins.readDir dir).${name};
        path = dir + "/${name}";
      in
      if entryType == "directory" then
        listTomlFiles path
      else if entryType == "regular" && lib.hasSuffix ".toml" name then
        [ path ]
      else
        [ ]
    ) (builtins.attrNames (builtins.readDir dir));

  loadEntry =
    isExplicit: configFile:
    let
      configData = builtins.fromTOML (builtins.readFile configFile);
      isNoop = !(configData.config ? runtime);
      deploy = configData.deploy or { };
      deployEnable = deploy.enable or false;
      deployTarget = deploy.target or "system";
      isActive = !isNoop && (isExplicit || (deployEnable && deployTarget == "system"));
      name =
        configData.name
          or (throw "services.podman-agent-container: TOML config must set top-level name: ${toString configFile}");
      runtimePackageNames = configData.config.runtime.packages or [ ];
      runtimePackages = map (
        packageName:
        if builtins.hasAttr packageName pkgs then
          builtins.getAttr packageName pkgs
        else
          throw "services.podman-agent-container: unknown package in ${toString configFile} config.runtime.packages: ${packageName}"
      ) runtimePackageNames;

      runtimeEnv = pkgs.buildEnv {
        name = "podman-agent-container-runtime-${name}";
        paths = runtimePackages;
      };

      minimalRootfs = pkgs.runCommand "podman-agent-container-rootfs-${name}" { } ''
                mkdir -p $out/{etc,tmp,run}
                cat > $out/etc/passwd <<'EOF'
        root:x:0:0:root:/root:/bin/sh
        EOF
                cat > $out/etc/group <<'EOF'
        root:x:0:
        EOF
      '';

      renderedQuadlet = pkgs.runCommand "${name}.container" { } ''
        export PATH=${runtimeEnv}/bin:$PATH
        ${lib.getExe' cfg.package "podman-agent-container"} render-nixos ${configFile} ${minimalRootfs} ${name} > $out
      '';
    in
    {
      inherit
        configFile
        configData
        deployEnable
        deployTarget
        isExplicit
        isNoop
        isActive
        name
        renderedQuadlet
        ;
    };

  discoveredConfigFiles = if cfg.configRoot == null then [ ] else listTomlFiles cfg.configRoot;
  entries = (map (loadEntry true) cfg.configFiles) ++ (map (loadEntry false) discoveredConfigFiles);
  activeEntries = builtins.filter (entry: entry.isActive) entries;
  activeNames = map (entry: entry.name) activeEntries;
  uniqueActiveNames = lib.unique activeNames;
in
{
  options.services.podman-agent-container = {
    enable = lib.mkEnableOption "TOML-driven Podman Quadlet containers";

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
        recursively. A discovered TOML file becomes a NixOS-managed container only
        when it is not no-op and sets `[deploy] enable = true` with `target = "system"`
        or no target.
      '';
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = activeNames == uniqueActiveNames;
        message = "services.podman-agent-container: active TOML config names must be unique.";
      }
    ];

    virtualisation.podman.enable = true;

    environment.etc = lib.listToAttrs (
      map (entry: {
        name = "containers/systemd/${entry.name}.container";
        value.source = entry.renderedQuadlet;
      }) activeEntries
    );
  };
}
