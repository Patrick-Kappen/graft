{
  lib,
  pkgs,
  materialised,
  producer,
  hostId,
  target,
  manager,
  workerApiRange,
  requiredBackend,
  lifecycleCapabilities ? [ ],
  observabilityCapabilities ? [ "manifest" ],
}:

let
  dependencyFields = [
    "requires"
    "wants"
    "after"
    "before"
    "partOf"
    "bindsTo"
  ];

  lowerLifecycle =
    resolved:
    let
      service = resolved.service or { };
      serviceType = service.type or "notify";
    in
    if serviceType == "notify" then
      "long_running"
    else if service.remainAfterExit or false then
      "setup"
    else
      "job";

  lowerWorkload =
    fact:
    let
      dependencies = fact.resolved.dependencies or { };
      network = fact.resolved.network or { };
      namespace = network.namespace or { };
      dependencyUnits =
        lib.concatMap (field: dependencies.${field} or [ ]) dependencyFields
        ++ lib.optional ((namespace.mode or null) == "container") namespace.unit;
      dependencyServices = lib.sort builtins.lessThan (
        lib.unique (
          map (unit: "${lib.removeSuffix ".container" unit}.service") (
            lib.filter (lib.hasSuffix ".container") dependencyUnits
          )
        )
      );
      # The record ID follows complete resolved intent. The remaining hashes
      # identify exact existing source, dependency, Quadlet, and closure facts;
      # manifest and endpoint digests remain exclusively Rust-owned.
      resolvedDigest = builtins.hashString "sha256" (builtins.toJSON fact.resolved);
    in
    {
      workloadId = resolvedDigest;
      name = fact.workloadName;
      target = fact.resolved.deploy.target;
      inherit (fact) enabled;
      lifecycle = lowerLifecycle fact.resolved;
      startupIntent = if fact.resolved ? install then "manager_target" else "disabled";
      inherit (fact)
        sourceIdentity
        quadletSourceUnit
        generatedService
        containerName
        ;
      sourceDigest = builtins.hashFile "sha256" fact.sourcePath;
      inherit resolvedDigest;
      dependencyDigest = builtins.hashString "sha256" (builtins.toJSON dependencies);
      artifactIdentity = builtins.hashFile "sha256" fact.quadletSource;
      rootfsStorePath = "${fact.rootfs}";
      closureIdentity = builtins.hashFile "sha256" "${fact.closureInfo}/store-paths";
      inherit
        dependencyServices
        lifecycleCapabilities
        observabilityCapabilities
        ;
      requiredWorkerApi = workerApiRange;
      requiredProducer = producer;
      inherit requiredBackend;
    };

  value = {
    inherit
      producer
      hostId
      target
      manager
      workerApiRange
      ;
    workloads = map (sourceIdentity: lowerWorkload materialised.manifestFacts.${sourceIdentity}) (
      builtins.attrNames materialised.manifestFacts
    );
  };
in
{
  inherit value;
  file = pkgs.writeText "graft-manifest-preimage.json" (builtins.toJSON value);
}
