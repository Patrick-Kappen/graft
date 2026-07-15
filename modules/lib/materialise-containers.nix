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
      LC_ALL=C sort -u ${packageClosure}/store-paths | while IFS= read -r store_path; do
        case "$store_path" in
          /nix/store/*) ;;
          *) echo "${optionName}: invalid closure path '$store_path' for container '${ctr.name}'" >&2; exit 1 ;;
        esac
        store_name="''${store_path#/nix/store/}"
        case "$store_name" in
          ""|*/*) echo "${optionName}: closure path '$store_path' is not a direct store child for container '${ctr.name}'" >&2; exit 1 ;;
        esac
        target_path="$out/nix/store/$store_name"
        if [ -L "$store_path" ]; then
          echo "${optionName}: top-level closure symlink '$store_path' is unsupported for container '${ctr.name}'" >&2
          exit 1
        elif [ -d "$store_path" ]; then
          mkdir "$target_path"
        elif [ -f "$store_path" ]; then
          touch "$target_path"
        else
          echo "${optionName}: unsupported closure object '$store_path' for container '${ctr.name}'" >&2
          exit 1
        fi
      done

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
      mount_lines=$(mktemp)

      {
        cat ${packageClosure}/store-paths
        printf '%s\n' ${lib.escapeShellArg "${env}"}
      } | LC_ALL=C sort -u > "$expected_paths"
      LC_ALL=C sort -u ${finalClosure}/store-paths > "$actual_paths"

      if ! cmp -s "$expected_paths" "$actual_paths"; then
        echo "${optionName}: final closure mismatch for container '${ctr.name}'" >&2
        diff -u "$expected_paths" "$actual_paths" >&2 || true
        exit 1
      fi

      member_count=$(wc -l < "$actual_paths")
      if [ "$member_count" -gt 512 ]; then
        echo "${optionName}: closure for container '${ctr.name}' has $member_count members; limit is 512; reduce config.runtime.packages" >&2
        exit 1
      fi

      printf 'Volume=%s/nix/store:/nix/store:ro,bind,nodev,nosuid\n' ${lib.escapeShellArg "${env}"} > "$mount_lines"

      while IFS= read -r store_path; do
        case "$store_path" in
          /nix/store/*) ;;
          *) echo "${optionName}: invalid final closure path '$store_path' for container '${ctr.name}'" >&2; exit 1 ;;
        esac
        store_name="''${store_path#/nix/store/}"
        case "$store_name" in
          ""|*/*) echo "${optionName}: final closure path '$store_path' is not a direct store child for container '${ctr.name}'" >&2; exit 1 ;;
        esac
        source_path="/nix/store/$store_name"
        target_path="${env}/nix/store/$store_name"
        if [ -L "$source_path" ]; then
          echo "${optionName}: top-level closure symlink '$source_path' is unsupported for container '${ctr.name}'" >&2
          exit 1
        elif [ -d "$source_path" ]; then
          if [ ! -d "$target_path" ] || [ -L "$target_path" ]; then
            echo "${optionName}: closure directory '$source_path' has no matching rootfs target for container '${ctr.name}'" >&2
            exit 1
          fi
        elif [ -f "$source_path" ]; then
          if [ ! -f "$target_path" ] || [ -L "$target_path" ]; then
            echo "${optionName}: closure file '$source_path' has no matching rootfs target for container '${ctr.name}'" >&2
            exit 1
          fi
        else
          echo "${optionName}: unsupported final closure object '$source_path' for container '${ctr.name}'" >&2
          exit 1
        fi
        printf 'Volume=%s:%s:ro,bind,nodev,nosuid\n' "$source_path" "$source_path" >> "$mount_lines"
      done < "$actual_paths"

      fragment_size=$(wc -c < "$mount_lines")
      if [ "$fragment_size" -gt 131072 ]; then
        echo "${optionName}: closure mount fragment for container '${ctr.name}' is $fragment_size bytes; limit is 131072; reduce config.runtime.packages" >&2
        exit 1
      fi

      if [ "$(grep -Fxc ${lib.escapeShellArg storeMountMarker} ${template})" -ne 1 ]; then
        echo "${optionName}: Quadlet template for container '${ctr.name}' lost its store-mount marker" >&2
        exit 1
      fi

      while IFS= read -r line; do
        if [ "$line" = ${lib.escapeShellArg storeMountMarker} ]; then
          cat "$mount_lines"
        else
          printf '%s\n' "$line"
        fi
      done < ${template} > "$out"
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
