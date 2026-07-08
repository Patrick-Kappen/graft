{ lib
, pkgs
, cfg
, target
, optionName
}:

let
  quadletRenderer = import ./render-quadlet.nix { inherit lib; };

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

  resolvedContainers = lib.mapAttrs
    (_: resolvedJson: builtins.fromJSON (builtins.readFile resolvedJson))
    resolvedJsonFiles;

  containers = lib.filterAttrs
    (_: ctr:
      (ctr.deploy.enable or true) && ctr.deploy.target == target
    )
    resolvedContainers;

  packageFor = containerName: package:
    if package == "graft-pause" then
      cfg.package
    else if builtins.hasAttr package pkgs then
      builtins.getAttr package pkgs
    else
      throw "${optionName}: unknown package '${package}' in container '${containerName}'";

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
    containers;

  quadletFiles = lib.mapAttrs
    (name: ctr:
      quadletRenderer.renderQuadletFile {
        inherit ctr;
        env = containerEnvs.${name};
      })
    containers;
in
{
  inherit containers quadletFiles;
}
