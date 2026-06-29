{
  lib,
  pkgs,
  package,
  configRoot,
  configFiles,
  deployTarget,
  optionPrefix,
}:

let
  tomlFormat = pkgs.formats.toml { };

  listTomlFiles =
    dir:
    lib.concatMap (
      name:
      let
        entryType = (builtins.readDir dir).${name};
        path = dir + "/${name}";
      in
      if entryType == "directory" then
        listTomlFiles path
      else if entryType == "regular" && lib.hasSuffix ".toml" name then
        [ path ]
      else
        [ ]
    ) (builtins.attrNames (builtins.readDir dir));

  readToml = configFile: builtins.fromTOML (builtins.readFile configFile);

  isEmptyValue =
    value:
    if builtins.isAttrs value then
      lib.all isEmptyValue (builtins.attrValues value)
    else if builtins.isList value then
      value == [ ]
    else
      false;

  mergeValues =
    path: left: right:
    if builtins.isAttrs left && builtins.isAttrs right then
      mergeAttrs path left right
    else if builtins.isList left && builtins.isList right then
      if
        path == [
          "runtime"
          "command"
        ]
      then
        right
      else
        lib.unique (left ++ right)
    else
      right;

  mergeAttrs =
    path: left: right:
    let
      keys = lib.unique ((builtins.attrNames left) ++ (builtins.attrNames right));
    in
    lib.genAttrs keys (
      key:
      if builtins.hasAttr key left && builtins.hasAttr key right then
        mergeValues (path ++ [ key ]) left.${key} right.${key}
      else if builtins.hasAttr key right then
        right.${key}
      else
        left.${key}
    );

  mergeConfigList = builtins.foldl' (acc: value: mergeAttrs [ ] acc value) { };

  applyPackageOps =
    configValue:
    let
      runtime = configValue.runtime or { };
      packageOps = runtime.packageOps or { };
      packages = runtime.packages or [ ];
      remove = packageOps.remove or [ ];
      add = packageOps.add or [ ];
      replace = packageOps.replace or [ ];
      replaceNames = map (item: item.name) replace;
      replaceWith = map (item: item."with") replace;
      filteredPackages = builtins.filter (
        packageName: !(builtins.elem packageName remove) && !(builtins.elem packageName replaceNames)
      ) packages;
      finalPackages = lib.unique (filteredPackages ++ replaceWith ++ add);
      runtimeWithoutOps = removeAttrs runtime [ "packageOps" ];
      runtimeWithPackages = runtimeWithoutOps // {
        packages = finalPackages;
      };
    in
    if configValue ? runtime then configValue // { runtime = runtimeWithPackages; } else configValue;

  requireConfigRoot =
    ref:
    if configRoot == null then
      throw "${optionPrefix}: graph ref ${ref} requires configRoot to be set"
    else
      configRoot;

  nodePath = ref: (requireConfigRoot ref) + "/${ref}.toml";

  relationRefs =
    relation:
    let
      setRefs = relation.set or [ ];
      addRefs = relation.add or [ ];
      removeRefs = relation.remove or [ ];
      refs = if setRefs != [ ] then setRefs else addRefs;
    in
    builtins.filter (ref: !(builtins.elem ref removeRefs)) refs;

  resolveConfigData =
    stack: configData:
    let
      parentRefs = relationRefs (configData.parents or { });
      childRefs = relationRefs (configData.children or { });
      parentConfigs = map (ref: resolveNode (stack ++ [ ref ]) ref) parentRefs;
      childConfigs = map (ref: resolveNode (stack ++ [ ref ]) ref) childRefs;
      selfConfig = configData.config or { };
    in
    mergeConfigList (parentConfigs ++ [ selfConfig ] ++ childConfigs);

  resolveNode =
    stack: ref:
    if builtins.elem ref (lib.init stack) then
      throw "${optionPrefix}: graph cycle detected: ${builtins.concatStringsSep " -> " stack}"
    else
      resolveConfigData stack (readToml (nodePath ref));

  loadEntry =
    isExplicit: configFile:
    let
      configData = readToml configFile;
      effectiveConfig = applyPackageOps (resolveConfigData [ ] configData);
      isNoop = isEmptyValue effectiveConfig;
      deploy = configData.deploy or { };
      deployEnable = deploy.enable or false;
      configDeployTarget = deploy.target or deployTarget;
      isActive = !isNoop && (isExplicit || (deployEnable && configDeployTarget == deployTarget));
      name =
        configData.name
          or (throw "${optionPrefix}: TOML config must set top-level name: ${toString configFile}");
      effectiveToml = tomlFormat.generate "podman-agent-container-effective-${name}.toml" {
        version = configData.version or 1;
        inherit name;
        config = effectiveConfig;
      };
      runtimePackageNames = effectiveConfig.runtime.packages or [ ];
      unknownPackageNames = builtins.filter (
        packageName: !(builtins.hasAttr packageName pkgs)
      ) runtimePackageNames;
      runtimePackages = map (packageName: builtins.getAttr packageName pkgs) runtimePackageNames;

      runtimeEnv = pkgs.buildEnv {
        name = "podman-agent-container-runtime-${name}";
        paths = runtimePackages;
      };

      minimalRootfs = pkgs.runCommand "podman-agent-container-rootfs-${name}" { } ''
                mkdir -p $out/{etc,tmp,run}
                cat > $out/etc/passwd <<'EOF'
        root:x:0:0:root:/root:/bin/sh
        EOF
                cat > $out/etc/group <<'EOF'
        root:x:0:
        EOF
      '';

      renderedQuadlet = pkgs.runCommand "${name}.container" { } ''
        export PATH=${runtimeEnv}/bin:$PATH
        ${lib.getExe' package "podman-agent-container"} render-nixos ${effectiveToml} ${minimalRootfs} ${name} > $out
      '';
    in
    {
      inherit
        configFile
        configData
        configDeployTarget
        deployEnable
        effectiveConfig
        effectiveToml
        isExplicit
        isNoop
        isActive
        name
        renderedQuadlet
        runtimePackageNames
        unknownPackageNames
        ;
    };

  discoveredConfigFiles = if configRoot == null then [ ] else listTomlFiles configRoot;
  entries = (map (loadEntry true) configFiles) ++ (map (loadEntry false) discoveredConfigFiles);
  activeEntries = builtins.filter (entry: entry.isActive) entries;
  activeNames = map (entry: entry.name) activeEntries;
  uniqueActiveNames = lib.unique activeNames;
in
{
  inherit
    activeEntries
    activeNames
    entries
    uniqueActiveNames
    ;
}
