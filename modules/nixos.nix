{ config, lib, pkgs, ... }:

let
  cfg = config.services.graft;

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

  systemContainers = lib.filterAttrs
    (_: ctr:
      (ctr.deploy.enable or true) && ctr.deploy.target == "system"
    )
    containers;

  packageFor = containerName: package:
    if package == "graft-pause" then
      cfg.package
    else if builtins.hasAttr package pkgs then
      builtins.getAttr package pkgs
    else
      throw "services.graft: unknown package '${package}' in container '${containerName}'";

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
    systemContainers;

  quadletFiles = lib.mapAttrs
    (name: ctr:
      let
        cmd = lib.escapeShellArgs ctr.runtime.command;
        env = containerEnvs.${name};
        container = ctr.container or { };
        hostname = container.hostname or null;
        user = container.user or null;
        group = container.group or null;
        workingDir = container.workingDir or null;
        environment = container.environment or { };
        environmentKeys = lib.sort builtins.lessThan (builtins.attrNames environment);
        environmentLines = lib.concatMapStrings
          (key: "Environment=${key}=${environment.${key}}\n")
          environmentKeys;
        environmentFile = container.environmentFile or [ ];
        environmentFileLines = lib.concatMapStrings
          (file: "EnvironmentFile=${file}\n")
          environmentFile;
        filesystem = ctr.filesystem or { };
        volumes = filesystem.volumes or [ ];
        volumeLines = lib.concatMapStrings
          (volume:
            let
              source = volume.source or null;
              target = volume.target;
              mode = volume.mode or null;
              mount =
                if source == null then
                  target
                else if mode == null then
                  "${source}:${target}"
                else
                  "${source}:${target}:${mode}";
            in
            "Volume=${mount}\n")
          volumes;
        network = ctr.network or { };
        publish = network.publish or [ ];
        publishLines = lib.concatMapStrings
          (port: "PublishPort=${port}\n")
          publish;
        service = ctr.service or { };
        restart = service.restart or null;
        restartSec = service.restartSec or null;
        timeoutStartSec = service.timeoutStartSec or null;
        timeoutStopSec = service.timeoutStopSec or null;
        serviceLines = lib.optionalString (restart != null) "Restart=${restart}\n"
          + lib.optionalString (restartSec != null) "RestartSec=${restartSec}\n"
          + lib.optionalString (timeoutStartSec != null) "TimeoutStartSec=${timeoutStartSec}\n"
          + lib.optionalString (timeoutStopSec != null) "TimeoutStopSec=${timeoutStopSec}\n";
        serviceSection = lib.optionalString (serviceLines != "") "\n[Service]\n${serviceLines}";
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
      + lib.optionalString (group != null) ''
        Group=${group}
      ''
      + lib.optionalString (workingDir != null) ''
        WorkingDir=${workingDir}
      ''
      + environmentLines
      + environmentFileLines
      + volumeLines
      + publishLines
      + serviceSection
    )
    systemContainers;

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

    environment.etc = lib.mapAttrs'
      (name: _:
        lib.nameValuePair
          "containers/systemd/${lib.removeSuffix ".toml" name}.container"
          { text = quadletFiles.${name}; }
      )
      systemContainers;

  };
}
