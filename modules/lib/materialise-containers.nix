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

  tomlFiles = builtins.listToAttrs (
    map (entry: lib.nameValuePair entry.name entry) checkedTomlEntries
  );

  contextLinks = lib.concatMapStrings (
    entry: "ln -s ${lib.escapeShellArg "${entry.path}"} context/${lib.escapeShellArg entry.name}\n"
  ) checkedTomlEntries;

  contextArgs = lib.concatMapStringsSep " " (
    entry: "--context context/${lib.escapeShellArg entry.name}"
  ) checkedTomlEntries;

  resolveToml =
    name: entry:
    let
      containerName = lib.removeSuffix ".toml" name;
    in
    pkgs.runCommand "graft-resolve-${containerName}" { } ''
      mkdir context
      ${contextLinks}
      ${lib.getExe' cfg.package "graft"} context/${lib.escapeShellArg name} ${contextArgs} > $out
    '';

  resolvedJsonFiles = lib.mapAttrs resolveToml tomlFiles;

  resolvedContainers = lib.mapAttrs (
    _: resolvedJson: builtins.fromJSON (builtins.readFile resolvedJson)
  ) resolvedJsonFiles;

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

  containerEnvs = lib.mapAttrs (
    _: ctr:
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
  ) containers;

  quadletFiles = lib.mapAttrs (
    name: ctr:
    quadletRenderer.renderQuadletFile {
      inherit ctr;
      env = containerEnvs.${name};
    }
  ) containers;
in
{
  inherit containers quadletFiles;
}
