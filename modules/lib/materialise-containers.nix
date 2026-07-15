{
  lib,
  pkgs,
  cfg,
  target,
  optionName,
}:

let
  quadletRenderer = import ./render-quadlet.nix { inherit lib; };

  configuredRoots = lib.optional (cfg.configRoot != null) cfg.configRoot ++ cfg.configRoots;

  duplicates =
    values:
    lib.unique (
      lib.filter (value: (lib.length (lib.filter (candidate: candidate == value) values)) > 1) values
    );

  tomlEntriesForRoot =
    root:
    map
      (name: {
        inherit name root;
        path = root + "/${name}";
      })
      (
        builtins.attrNames (
          lib.filterAttrs (name: type: type == "regular" && lib.hasSuffix ".toml" name) (
            builtins.readDir root
          )
        )
      );

  tomlEntries = lib.concatMap tomlEntriesForRoot configuredRoots;
  duplicateTomlNames = duplicates (map (entry: entry.name) tomlEntries);
  checkedTomlEntries =
    if duplicateTomlNames == [ ] then
      tomlEntries
    else
      throw "${optionName}: duplicate container TOML filename(s): ${lib.concatStringsSep ", " duplicateTomlNames}";

  contextLinks = lib.concatMapStrings (
    entry: "ln -s ${lib.escapeShellArg "${entry.path}"} context/${lib.escapeShellArg entry.name}\n"
  ) checkedTomlEntries;

  setArgs = lib.concatMapStringsSep " " (
    entry: "context/${lib.escapeShellArg entry.name}"
  ) checkedTomlEntries;

  resolvedJsonFile =
    if checkedTomlEntries == [ ] then
      null
    else
      pkgs.runCommand "graft-resolve-set" { } ''
        mkdir context
        ${contextLinks}
        ${lib.getExe' cfg.package "graft"} --set ${setArgs} > $out
      '';

  resolvedContainers =
    if resolvedJsonFile == null then { } else builtins.fromJSON (builtins.readFile resolvedJsonFile);

  targetContainers = lib.filterAttrs (
    _: ctr: (ctr.deploy.enable or true) && ctr.deploy.target == target
  ) resolvedContainers;

  duplicateContainerNames = duplicates (map (ctr: ctr.name) (builtins.attrValues targetContainers));
  containers =
    if duplicateContainerNames == [ ] then
      targetContainers
    else
      throw "${optionName}: duplicate container name(s) for target '${target}': ${lib.concatStringsSep ", " duplicateContainerNames}";

  packageFor =
    containerName: package:
    if package == "graft-pause" then
      cfg.package
    else if builtins.hasAttr package pkgs then
      builtins.getAttr package pkgs
    else
      throw "${optionName}: unknown package '${package}' in container '${containerName}'";

  containerInners = lib.mapAttrs (
    _: ctr:
    pkgs.buildEnv {
      name = "graft-${ctr.name}-inner";
      paths = map (packageFor ctr.name) ctr.runtime.packages;
      ignoreCollisions = true;
    }
  ) containers;

  packageClosures = lib.mapAttrs (
    name: _: pkgs.closureInfo { rootPaths = [ containerInners.${name} ]; }
  ) containers;

  containerEnvs = lib.mapAttrs (
    name: ctr:
    let
      inner = containerInners.${name};
      packageClosure = packageClosures.${name};
    in
    pkgs.runCommand "graft-${ctr.name}-env" { } ''
      set -euo pipefail

      # Real system directories and runtime mountpoints.
      mkdir -p $out/{etc,tmp,var/tmp,home,root,run,proc,sys,dev,nix/store}
      ln -s /proc/mounts $out/etc/mtab
      touch $out/etc/hostname $out/etc/hosts $out/etc/resolv.conf
      touch $out/run/.containerenv

      # Symlink everything from the inner env except directories we own.
      for entry in ${inner}/*; do
        entry_name=$(basename "$entry")
        case "$entry_name" in
          etc|tmp|var|home|root|run|proc|sys|dev|nix) continue ;;
        esac
        ln -s "$entry" "$out/$entry_name"
      done

      # Copy /etc contents from packages (if any) into our real /etc.
      if [ -e ${inner}/etc ]; then
        cp -rL ${inner}/etc/. $out/etc/ 2>/dev/null || true
      fi

      # OCI runtimes require every nested bind target before applying ReadOnly=.
      ${pkgs.bash}/bin/bash ${./prepare-closure-targets.sh} \
        ${packageClosure}/store-paths \
        "$out" \
        ${lib.escapeShellArg optionName} \
        ${lib.escapeShellArg ctr.name}

      # The final rootfs is itself a directory member of its runtime closure.
      mkdir "$out/nix/store/$(basename "$out")"
    ''
  ) containers;

  finalClosures = lib.mapAttrs (
    name: _: pkgs.closureInfo { rootPaths = [ containerEnvs.${name} ]; }
  ) containers;

  storeMountMarker = "@GRAFT_STORE_MOUNTS@";

  quadletTemplates = lib.mapAttrs (
    name: ctr:
    pkgs.writeText "graft-${ctr.name}-quadlet-template" (
      quadletRenderer.renderQuadletFile {
        inherit ctr;
        env = containerEnvs.${name};
        storeMountLines = storeMountMarker;
      }
    )
  ) containers;

  quadletFiles = lib.mapAttrs (
    name: ctr:
    let
      env = containerEnvs.${name};
      packageClosure = packageClosures.${name};
      finalClosure = finalClosures.${name};
      template = quadletTemplates.${name};
    in
    pkgs.runCommand "graft-${ctr.name}.container" { } ''
      set -euo pipefail

      expected_paths=$(mktemp)
      actual_paths=$(mktemp)

      {
        cat ${packageClosure}/store-paths
        printf '%s\n' ${lib.escapeShellArg "${env}"}
      } | LC_ALL=C sort -u > "$expected_paths"
      LC_ALL=C sort -u ${finalClosure}/store-paths > "$actual_paths"

      ${pkgs.bash}/bin/bash ${./check-closure-equality.sh} \
        "$expected_paths" \
        "$actual_paths" \
        ${lib.escapeShellArg optionName} \
        ${lib.escapeShellArg ctr.name}

      ${pkgs.bash}/bin/bash ${./render-closure-mounts.sh} \
        "$actual_paths" \
        ${lib.escapeShellArg "${env}"} \
        ${template} \
        "$out" \
        ${lib.escapeShellArg optionName} \
        ${lib.escapeShellArg ctr.name} \
        ${lib.escapeShellArg storeMountMarker} \
        ${./check-closure-limits.sh}
    ''
  ) containers;
in
{
  inherit
    containers
    containerEnvs
    finalClosures
    quadletFiles
    ;
}
