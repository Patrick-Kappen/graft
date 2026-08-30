{
  config,
  lib,
  pkgs,
  ...
}:

let
  cfg = config.services.graft;

  producerBuildId = lib.attrByPath [ "graftBuildId" ] "source" cfg.package;
  requiredWorkerApiRange = import ../nix/worker-api-range.nix;

  canonicalUuidV7 = lib.types.strMatching "[0-9a-f]{8}-[0-9a-f]{4}-7[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}";

  materialised = import ./lib/materialise-containers.nix {
    inherit lib pkgs cfg;
    target = "system";
    optionName = "services.graft";
  };

  manifestPublication =
    if cfg.package == null || cfg.hostId == null then
      null
    else
      import ./lib/render-manifest-generation.nix {
        inherit lib pkgs materialised;
        inherit (cfg) package hostId;
        producer = {
          name = "graft";
          version = lib.getVersion cfg.package;
          buildId = producerBuildId;
        };
        target = "system";
        manager = "system";
        workerApiRange = requiredWorkerApiRange;
        requiredBackend = {
          runtime = "podman";
          minimumVersion = "5.0.0";
        };
        name = "graft-system-manifest-generation";
      };

  worker = lib.getExe' cfg.package "graft-worker";
  workerApiRange = lib.attrByPath [ "graftWorkerApiRange" ] null cfg.package;

in
{
  options.services.graft = {
    enable = lib.mkEnableOption "Graft — TOML-driven Podman Quadlet containers";

    package = lib.mkOption {
      type = lib.types.nullOr lib.types.package;
      default = null;
      description = "Graft package providing the resolver, graft-pause, and internal manifest publication helpers.";
    };

    hostId = lib.mkOption {
      type = lib.types.nullOr canonicalUuidV7;
      default = null;
      example = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20";
      description = "Stable non-secret host identity as a canonical lowercase RFC 9562 UUIDv7.";
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
  };

  config = lib.mkIf cfg.enable (
    lib.mkMerge [
      {
        assertions = [
          {
            assertion = cfg.package != null;
            message = "services.graft.package must be set when services.graft.enable is true.";
          }
          {
            assertion = cfg.hostId != null;
            message = "services.graft.hostId must be set to a canonical lowercase UUIDv7 when services.graft.enable is true.";
          }
          {
            assertion =
              workerApiRange != null
              && workerApiRange.major == requiredWorkerApiRange.major
              && workerApiRange.min_minor <= requiredWorkerApiRange.min_minor
              && workerApiRange.max_minor >= requiredWorkerApiRange.max_minor;
            message = "services.graft.package has no compatible worker API range.";
          }
        ];

        environment.etc = lib.mapAttrs' (
          name: _:
          lib.nameValuePair "containers/systemd/${lib.removeSuffix ".toml" name}.container" {
            source = materialised.quadletFiles.${name};
          }
        ) materialised.containers;

        users.groups.graft = { };

        systemd = {
          tmpfiles.rules = [
            "d /run/graft 0755 root root - -"
            "d /run/graft/system 0750 root graft - -"
          ];

          sockets.graft-system-worker = {
            wantedBy = [ "sockets.target" ];
            socketConfig = {
              ListenStream = "/run/graft/system/worker.sock";
              SocketMode = "0660";
              SocketUser = "root";
              SocketGroup = "graft";
              RemoveOnStop = true;
            };
          };

          services.graft-system-worker = lib.mkIf (cfg.package != null && cfg.hostId != null) {
            description = "Graft system worker";
            unitConfig = {
              StartLimitIntervalSec = "60s";
              StartLimitBurst = 5;
            };
            serviceConfig = {
              ExecStart = "${pkgs.bash}/bin/bash -c 'gid=\$(${pkgs.getent}/bin/getent group graft | ${pkgs.coreutils}/bin/cut -d: -f3); [[ \"\$gid\" =~ ^[0-9]+$ ]] && exec ${worker} --target system --effective-uid 0 --manager system --graft-gid \"\$gid\" --producer-name graft --producer-version ${lib.escapeShellArg (lib.getVersion cfg.package)} --producer-build-id ${lib.escapeShellArg producerBuildId}'";
              User = "root";
              Group = "root";
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
              ProtectProc = "invisible";
              ProcSubset = "pid";
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
        };
      }

      (lib.mkIf (cfg.package != null && cfg.hostId != null) {
        system.build.graftManifestGeneration = manifestPublication.generation;

        system.activationScripts.graftManifestPublication = {
          deps = [
            "users"
            "groups"
            "etc"
          ];
          text = ''
            graft_group_record="$(getent group graft)" || {
              echo "services.graft: fixed graft group is unavailable during manifest publication" >&2
              exit 1
            }
            IFS=: read -r graft_group_name _ graft_group_gid _ <<< "$graft_group_record"
            if [[ "$graft_group_name" != graft || ! "$graft_group_gid" =~ ^[0-9]+$ ]]; then
              echo "services.graft: fixed graft group has an invalid runtime identity" >&2
              exit 1
            fi

            ${lib.getExe' cfg.package "graft-manifest-publish-system"} \
              ${manifestPublication.generation} \
              --graft-gid "$graft_group_gid" \
              --producer-name graft \
              --producer-version ${lib.escapeShellArg (lib.getVersion cfg.package)} \
              --producer-build-id ${lib.escapeShellArg producerBuildId}
          '';
        };
      })
    ]
  );
}
