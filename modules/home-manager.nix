{ config, lib, pkgs, ... }:

let
  cfg = config.programs.graft;

  tomlFiles = lib.optionalAttrs (cfg.configRoot != null)
    (lib.filterAttrs
      (name: type: type == "regular" && lib.hasSuffix ".toml" name)
      (builtins.readDir cfg.configRoot));

  resolveToml = name: _:
    let
      containerName = lib.removeSuffix ".toml" name;
      tomlFile = cfg.configRoot + "/${name}";
    in
    pkgs.runCommand "graft-resolve-${containerName}" { } ''
      ${lib.getExe' cfg.package "graft"} ${tomlFile} > $out
    '';

  resolvedJsonFiles = lib.mapAttrs resolveToml tomlFiles;

  containers = lib.mapAttrs
    (_: resolvedJson: builtins.fromJSON (builtins.readFile resolvedJson))
    resolvedJsonFiles;

  userContainers = lib.filterAttrs
    (_: ctr:
      (ctr.deploy.enable or true) && ctr.deploy.target == "user"
    )
    containers;

  packageFor = containerName: package:
    if package == "graft-pause" then
      cfg.package
    else if builtins.hasAttr package pkgs then
      builtins.getAttr package pkgs
    else
      throw "programs.graft: unknown package '${package}' in container '${containerName}'";

  containerEnvs = lib.mapAttrs
    (_: ctr:
      let
        inner = pkgs.buildEnv {
          name = "graft-${ctr.name}-inner";
          paths = map (packageFor ctr.name) ctr.runtime.packages;
          ignoreCollisions = true;
        };
      in
      pkgs.runCommand "graft-${ctr.name}-env" { } ''
        # Real system directories (so overlay can write to them)
        mkdir -p $out/{etc,tmp,var,home,root,run,proc,sys,dev}
        # Mount points required by crun/Podman at container start
        ln -s /proc/mounts $out/etc/mtab
        touch $out/etc/hostname $out/etc/hosts $out/etc/resolv.conf
        touch $out/run/.containerenv

        # Symlink everything from the inner env except directories we own
        for entry in ${inner}/*; do
          name=$(basename "$entry")
          case "$name" in
            etc|tmp|var|home|root|run|proc|sys|dev) continue ;;
          esac
          ln -s "$entry" "$out/$name"
        done

        # Copy /etc contents from packages (if any) into our real /etc
        if [ -e ${inner}/etc ]; then
          cp -rL ${inner}/etc/. $out/etc/ 2>/dev/null || true
        fi
      ''
    )
    userContainers;

  quadletFiles = lib.mapAttrs
    (name: ctr:
      let
        cmd = lib.escapeShellArgs ctr.runtime.command;
        env = containerEnvs.${name};
        container = ctr.container or { };
        hostname = container.hostname or null;
        user = container.user or null;
        workingDir = container.workingDir or null;
        environment = container.environment or { };
        environmentKeys = lib.sort builtins.lessThan (builtins.attrNames environment);
        environmentLines = lib.concatMapStrings
          (key: "Environment=${key}=${environment.${key}}\n")
          environmentKeys;
        service = ctr.service or { };
        restart = service.restart or null;
      in
      ''
        [Container]
        ContainerName=${ctr.name}
        Rootfs=${env}:O
        Exec=${cmd}
        Volume=/nix/store:/nix/store:ro
      ''
      + lib.optionalString (hostname != null) ''
        HostName=${hostname}
      ''
      + lib.optionalString (user != null) ''
        User=${user}
      ''
      + lib.optionalString (workingDir != null) ''
        WorkingDir=${workingDir}
      ''
      + environmentLines
      + lib.optionalString (restart != null) ''

        [Service]
        Restart=${restart}
      ''
    )
    userContainers;

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
          { text = quadletFiles.${name}; }
      )
      userContainers;

  };
}
