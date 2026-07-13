{ lib }:

let
  escapeSystemdExecArg = value: lib.replaceStrings [ "%" "$" ] [ "%%" "$$" ] (toString value);

  escapeSystemdQuotedExecArg =
    value: lib.replaceStrings [ "\\" "\"" "%" "$" ] [ "\\\\" "\\\"" "%%" "$$" ] (toString value);

  quoteSystemdExecArg = value: "\"${escapeSystemdQuotedExecArg value}\"";

  renderQuadletFile =
    { ctr, env }:
    let
      cmd = lib.concatStringsSep " " (map quoteSystemdExecArg ctr.runtime.command);
      dependencies = ctr.dependencies or { };
      requires = dependencies.requires or [ ];
      wants = dependencies.wants or [ ];
      after = dependencies.after or [ ];
      before = dependencies.before or [ ];
      partOf = dependencies.partOf or [ ];
      bindsTo = dependencies.bindsTo or [ ];
      unitLines =
        lib.optionalString (requires != [ ]) "Requires=${lib.concatStringsSep " " requires}\n"
        + lib.optionalString (wants != [ ]) "Wants=${lib.concatStringsSep " " wants}\n"
        + lib.optionalString (after != [ ]) "After=${lib.concatStringsSep " " after}\n"
        + lib.optionalString (before != [ ]) "Before=${lib.concatStringsSep " " before}\n"
        + lib.optionalString (partOf != [ ]) "PartOf=${lib.concatStringsSep " " partOf}\n"
        + lib.optionalString (bindsTo != [ ]) "BindsTo=${lib.concatStringsSep " " bindsTo}\n";
      unitSection = lib.optionalString (unitLines != "") "[Unit]\n${unitLines}\n";
      container = ctr.container or { };
      hostname = container.hostname or null;
      user = container.user or null;
      group = container.group or null;
      workingDir = container.workingDir or null;
      environment = container.environment or { };
      environmentKeys = builtins.attrNames environment;
      environmentLines = lib.concatMapStrings (
        key:
        let
          assignment = "${key}=${environment.${key}}";
        in
        "Environment=\"${escapeSystemdQuotedExecArg assignment}\"\n"
      ) environmentKeys;
      environmentFile = container.environmentFile or [ ];
      environmentFileLines = lib.concatMapStrings (
        file: "EnvironmentFile=${quoteSystemdExecArg file}\n"
      ) environmentFile;
      filesystem = ctr.filesystem or { };
      volumes = filesystem.volumes or [ ];
      volumeLines = lib.concatMapStrings (
        volume:
        let
          source = volume.source or null;
          inherit (volume) target;
          mode = volume.mode or null;
          mount =
            if source == null then
              target
            else if mode == null then
              "${source}:${target}"
            else
              "${source}:${target}:${mode}";
        in
        "Volume=${escapeSystemdExecArg mount}\n"
      ) volumes;
      devices = filesystem.devices or [ ];
      deviceLines = lib.concatMapStrings (
        device: "AddDevice=${escapeSystemdExecArg device.source}\n"
      ) devices;
      network = ctr.network or { };
      namespace = network.namespace or null;
      networkLine =
        if namespace == null then
          ""
        else if namespace.mode == "none" then
          "Network=none\n"
        else if namespace.mode == "container" then
          "Network=${escapeSystemdExecArg namespace.unit}\n"
        else
          throw "unsupported resolved network namespace mode '${namespace.mode}'";
      publish = network.publish or [ ];
      publishLines = lib.concatMapStrings (port: "PublishPort=${escapeSystemdExecArg port}\n") publish;
      service = ctr.service or { };
      serviceType = service.type or null;
      remainAfterExit = service.remainAfterExit or null;
      restart = service.restart or null;
      restartSec = service.restartSec or null;
      timeoutStartSec = service.timeoutStartSec or null;
      timeoutStopSec = service.timeoutStopSec or null;
      serviceLines =
        lib.optionalString (serviceType != null) "Type=${toString serviceType}\n"
        +
          lib.optionalString (remainAfterExit != null)
            "RemainAfterExit=${if remainAfterExit then "yes" else "no"}\n"
        + lib.optionalString (restart != null) "Restart=${toString restart}\n"
        + lib.optionalString (restartSec != null) "RestartSec=${toString restartSec}\n"
        + lib.optionalString (timeoutStartSec != null) "TimeoutStartSec=${toString timeoutStartSec}\n"
        + lib.optionalString (timeoutStopSec != null) "TimeoutStopSec=${toString timeoutStopSec}\n";
      serviceSection = lib.optionalString (serviceLines != "") "\n[Service]\n${serviceLines}";
      install = ctr.install or { };
      wantedBy = install.wantedBy or null;
      installSection = lib.optionalString (
        wantedBy != null
      ) "\n[Install]\nWantedBy=${toString wantedBy}\n";
    in
    unitSection
    + ''
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
    + deviceLines
    + networkLine
    + publishLines
    + serviceSection
    + installSection;
in
{
  inherit renderQuadletFile;
}
