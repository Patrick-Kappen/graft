{ lib }:

let
  escapeSystemdQuoted = value:
    lib.replaceStrings [ "\\" "\"" "%" ] [ "\\\\" "\\\"" "%%" ] value;

  renderQuadletFile = { ctr, env }:
    let
      cmd = lib.escapeShellArgs ctr.runtime.command;
      container = ctr.container or { };
      hostname = container.hostname or null;
      user = container.user or null;
      group = container.group or null;
      workingDir = container.workingDir or null;
      environment = container.environment or { };
      environmentKeys = lib.sort builtins.lessThan (builtins.attrNames environment);
      environmentLines = lib.concatMapStrings
        (key:
          let assignment = "${key}=${environment.${key}}";
          in "Environment=\"${escapeSystemdQuoted assignment}\"\n")
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
    + serviceSection;
in
{
  inherit renderQuadletFile;
}
