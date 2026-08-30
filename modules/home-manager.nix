{
  config,
  lib,
  pkgs,
  options,
  ...
}:

let
  cfg = config.programs.graft;
  osConfig = config._module.args.osConfig or null;
  inheritedHostId =
    if osConfig == null then null else lib.attrByPath [ "services" "graft" "hostId" ] null osConfig;
  effectiveHostId = if inheritedHostId != null then inheritedHostId else cfg.hostId;
  hasHomeActivation = builtins.hasAttr "home" options;
  configHome = lib.attrByPath [ "xdg" "configHome" ] "/home/test/.config" config;
  producerBuildId = lib.attrByPath [ "graftBuildId" ] "source" cfg.package;
  canonicalUuidV7 = lib.types.strMatching "[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}";

  manifestPublication =
    if cfg.package == null || effectiveHostId == null then
      null
    else
      import ./lib/render-manifest-generation.nix {
        inherit lib pkgs materialised;
        inherit (cfg) package;
        hostId = effectiveHostId;
        producer = {
          name = "graft";
          version = lib.getVersion cfg.package;
          buildId = producerBuildId;
        };
        target = "user";
        manager = "user";
        workerApiRange = {
          major = 1;
          min_minor = 0;
          max_minor = 0;
        };
        requiredBackend = {
          runtime = "podman";
          minimumVersion = "5.0.0";
        };
        name = "graft-user-manifest-generation";
      };

  worker = lib.getExe' cfg.package "graft-worker";
  workerApiRange = lib.attrByPath [ "graftWorkerApiRange" ] null cfg.package;

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

    configRoots = lib.mkOption {
      type = lib.types.listOf lib.types.path;
      default = [ ];
      description = "Additional directories containing .toml container definitions, in order.";
    };

    hostId = lib.mkOption {
      type = lib.types.nullOr canonicalUuidV7;
      default = inheritedHostId;
      example = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20";
      description = "Stable non-secret host identity, inherited from NixOS when available.";
    };
  };

  config = lib.mkIf cfg.enable {
    assertions = [
      {
        assertion = (cfg.configRoot == null && cfg.configRoots == [ ]) || cfg.package != null;
        message = "programs.graft.package must be set when programs.graft.configRoot or programs.graft.configRoots is set.";
      }
      {
        assertion = inheritedHostId == null || cfg.hostId == null || cfg.hostId == inheritedHostId;
        message = "programs.graft.hostId must match services.graft.hostId when Home Manager is integrated with NixOS.";
      }
      {
        assertion = cfg.package != null;
        message = "programs.graft.package must be set when programs.graft.enable is true.";
      }
      {
        assertion = cfg.package != null && builtins.pathExists "${cfg.package}/bin/graft-worker";
        message = "programs.graft.package must install graft-worker for worker integration.";
      }
      {
        assertion =
          workerApiRange != null
          && workerApiRange.major == 1
          && workerApiRange.min_minor <= 0
          && workerApiRange.max_minor >= 0;
        message = "programs.graft.package has no compatible worker API range.";
      }
      {
        assertion = !hasHomeActivation || effectiveHostId != null;
        message = "programs.graft.hostId must be set to a canonical lowercase UUIDv7 when programs.graft.enable is true.";
      }
    ];

    systemd.user.tmpfiles.rules = lib.mkIf (lib.hasAttrByPath [ "systemd" "user" ] config) [
      "d %t/graft 0700 %u %g - -"
      "d %t/graft/user 0700 %u %g - -"
      "d %t/graft/user/interlocks 0700 %u %g - -"
    ];

    systemd.user.sockets.graft-user-worker = lib.mkIf (lib.hasAttrByPath [ "systemd" "user" ] config) {
      wantedBy = [ "sockets.target" ];
      socketConfig = {
        ListenStream = "%t/graft/user/worker.sock";
        SocketMode = "0600";
        RemoveOnStop = true;
      };
    };

    systemd.user.services.graft-user-worker = lib.mkIf (lib.hasAttrByPath [ "systemd" "user" ] config) {
      description = "Graft user worker";
      unitConfig = {
        StartLimitIntervalSec = "60s";
        StartLimitBurst = 5;
      };
      serviceConfig = {
        ExecStart = "${worker} --target user --effective-uid %U --manager user --config-home ${lib.escapeShellArg configHome} --producer-name graft --producer-version ${lib.escapeShellArg (lib.getVersion cfg.package)} --producer-build-id ${lib.escapeShellArg producerBuildId}";
        Restart = "on-failure";
        RestartSec = "2s";
        NoNewPrivileges = true;
        PrivateTmp = true;
        ProtectSystem = "strict";
        ProtectHome = "read-only";
        ProtectKernelTunables = true;
        ProtectKernelModules = true;
        ProtectKernelLogs = true;
        ProtectControlGroups = true;
        ProtectClock = true;
        ProtectHostname = true;
        RestrictSUIDSGID = true;
        LockPersonality = true;
        MemoryDenyWriteExecute = true;
        RestrictRealtime = true;
        RestrictNamespaces = true;
        SystemCallArchitectures = "native";
        RestrictAddressFamilies = [ "AF_UNIX" ];
        UMask = "0077";
      };
    };

    xdg.configFile = lib.mapAttrs' (
      name: _:
      lib.nameValuePair "containers/systemd/${lib.removeSuffix ".toml" name}.container" {
        source = materialised.quadletFiles.${name};
      }
    ) materialised.containers;

    home.activation = lib.mkIf (hasHomeActivation && effectiveHostId != null) {
      graftManifestPublication =
        if manifestPublication == null then
          ""
        else
          (if builtins.hasAttr "hm" lib then lib.hm.dag.entryAfter [ "writeBoundary" ] else lib.id) ''
            ${lib.getExe' cfg.package "graft-manifest-publish-user"} \
              ${manifestPublication.generation} \
              --config-home ${lib.escapeShellArg config.xdg.configHome} \
              --state-home ${lib.escapeShellArg config.xdg.stateHome} \
              --producer-name graft \
              --producer-version ${lib.escapeShellArg (lib.getVersion cfg.package)} \
              --producer-build-id ${lib.escapeShellArg producerBuildId}
          '';
    };
  };
}
