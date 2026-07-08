{ lib }:

let
  escapeSystemdExecArg = value:
    lib.replaceStrings [ "%" "$" ] [ "%%" "$$" ] (toString value);

  escapeSystemdQuotedExecArg = value:
    lib.replaceStrings [ "\\" "\"" "%" "$" ] [ "\\\\" "\\\"" "%%" "$$" ] (toString value);

  quoteSystemdExecArg = value:
    "\"${escapeSystemdQuotedExecArg value}\"";

  renderQuadletFile = { ctr, env }:
    let
      cmd = lib.concatStringsSep " " (map quoteSystemdExecArg ctr.runtime.command);
      container = ctr.container or { };
      hostname = container.hostname or null;
      user = container.user or null;
      group = container.group or null;
      workingDir = container.workingDir or null;
      environment = container.environment or { };
      environmentKeys = builtins.attrNames environment;
      environmentLines = lib.concatMapStrings
        (key:
          let assignment = "${key}=${environment.${key}}";
          in "Environment=\"${escapeSystemdQuotedExecArg assignment}\"\n")
        environmentKeys;
      environmentFile = container.environmentFile or [ ];
      environmentFileLines = lib.concatMapStrings
        (file: "EnvironmentFile=${quoteSystemdExecArg file}\n")
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
          "Volume=${escapeSystemdExecArg mount}\n")
        volumes;
      network = ctr.network or { };
      publish = network.publish or [ ];
      publishLines = lib.concatMapStrings
        (port: "PublishPort=${escapeSystemdExecArg port}\n")
        publish;
      service = ctr.service or { };
      restart = service.restart or null;
      restartSec = service.restartSec or null;
      timeoutStartSec = service.timeoutStartSec or null;
      timeoutStopSec = service.timeoutStopSec or null;
      serviceLines = lib.optionalString (restart != null) "Restart=${toString restart}\n"
        + lib.optionalString (restartSec != null) "RestartSec=${toString restartSec}\n"
        + lib.optionalString (timeoutStartSec != null) "TimeoutStartSec=${toString timeoutStartSec}\n"
        + lib.optionalString (timeoutStopSec != null) "TimeoutStopSec=${toString timeoutStopSec}\n";
      serviceSection = lib.optionalString (serviceLines != "") "\n[Service]\n${serviceLines}";
    in
    ''
      [Container]
      ContainerName=${escapeSystemdExecArg ctr.name}
      Rootfs=${escapeSystemdExecArg env}:O
      Exec=${cmd}
      Volume=/nix/store:/nix/store:ro
    ''
    + lib.optionalString (hostname != null) ''
      HostName=${escapeSystemdExecArg hostname}
    ''
    + lib.optionalString (user != null) ''
      User=${escapeSystemdExecArg user}
    ''
    + lib.optionalString (group != null) ''
      Group=${escapeSystemdExecArg group}
    ''
    + lib.optionalString (workingDir != null) ''
      WorkingDir=${escapeSystemdExecArg workingDir}
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
