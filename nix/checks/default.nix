{
  self,
  system,
  pkgs,
  homeManager,
  graftPackage,
  graftVersion,
  graftBuildId,
  graftWorkerApiRange,
  buildIdForRevision,
  requireVersionParity,
}:
let
  inherit (builtins.seq system pkgs) lib;

  closureRegularFile = pkgs.writeText "graft-closure-regular-file" "regular-file\n";
  closureSymlink = pkgs.runCommand "graft-closure-symlink" { } ''
    ln -s ${closureRegularFile} $out
  '';
  closureFixturePackage = pkgs.runCommand "graft-closure-fixture" { } ''
    mkdir -p $out/share
    ln -s ${closureRegularFile} $out/share/regular-file
  '';
  rootfsEtcFixturePackage = pkgs.runCommand "graft-rootfs-etc-fixture" { } ''
    mkdir -p $out/etc/graft-test
    echo materialised > $out/etc/graft-test/config
  '';
  collisionPackageA = lib.meta.hiPrio (pkgs.writeTextDir "bin/graft-collision" "first\n");
  collisionPackageB = lib.meta.lowPrio (pkgs.writeTextDir "bin/graft-collision" "second\n");
  collisionTestPkgs = pkgs.extend (
    _: _: {
      graftCollisionFirst = collisionPackageA;
      graftCollisionSecond = collisionPackageB;
    }
  );
  collisionMaterialised = import ../../modules/lib/materialise-containers.nix {
    inherit lib;
    pkgs = collisionTestPkgs;
    cfg = {
      package = graftPackage;
      configRoot = ../../tests/nix/rootfs-collision;
      configRoots = [ ];
    };
    target = "system";
    optionName = "services.graft";
  };
  collisionFailure =
    pkgs.testers.testBuildFailure
      collisionMaterialised.containerInners."collision-system.toml";

  manifestMaterialisedFor =
    target:
    import ../../modules/lib/materialise-containers.nix {
      inherit lib pkgs target;
      cfg = {
        package = graftPackage;
        configRoot = ../../tests/nix/containers;
        configRoots = [
          ../../tests/nix/containers-extra
          ../../tests/nix/dependencies
        ];
      };
      optionName = if target == "system" then "services.graft" else "programs.graft";
    };
  manifestHostId = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20";
  manifestBuildId = graftPackage.graftBuildId or "source";
  manifestProducer = {
    name = "graft";
    version = graftVersion;
    buildId = "graft-test-build";
  };
  manifestWorkerApiRange = graftWorkerApiRange;
  manifestRequiredBackend = {
    runtime = "podman";
    minimumVersion = "5.0.0";
  };
  lowerManifestInput =
    target: materialised:
    import ../../modules/lib/lower-manifest-input.nix {
      inherit
        lib
        pkgs
        materialised
        target
        ;
      producer = manifestProducer;
      hostId = manifestHostId;
      manager = target;
      workerApiRange = manifestWorkerApiRange;
      requiredBackend = manifestRequiredBackend;
    };
  systemManifestMaterialised = manifestMaterialisedFor "system";
  userManifestMaterialised = manifestMaterialisedFor "user";
  systemManifestInput = lowerManifestInput "system" systemManifestMaterialised;
  userManifestInput = lowerManifestInput "user" userManifestMaterialised;
  renderManifestInput =
    name: input:
    pkgs.runCommand name { } ''
      ${lib.getExe' graftPackage "graft-manifest-render"} ${input} "$out"
    '';
  systemManifestDocuments = renderManifestInput "graft-system-manifest-documents" systemManifestInput.file;
  userManifestDocuments = renderManifestInput "graft-user-manifest-documents" userManifestInput.file;
  updateManifestWorkload =
    name: update: value:
    value
    // {
      workloads = map (
        workload: if workload.name == name then update workload else workload
      ) value.workloads;
    };
  systemManifestRecord =
    name:
    lib.findFirst (
      workload: workload.name == name
    ) (throw "missing system manifest test workload '${name}'") systemManifestInput.value.workloads;
  incompleteManifestInput = pkgs.writeText "graft-incomplete-manifest-input.json" (
    builtins.toJSON (
      updateManifestWorkload "nix-check-system" (
        workload: builtins.removeAttrs workload [ "rootfsStorePath" ]
      ) systemManifestInput.value
    )
  );
  duplicateManifestInput = pkgs.writeText "graft-duplicate-manifest-input.json" (
    builtins.toJSON (
      updateManifestWorkload "nix-check-plain-system" (
        workload: workload // { inherit (systemManifestRecord "nix-check-system") workloadId; }
      ) systemManifestInput.value
    )
  );
  mismatchedManifestInput = pkgs.writeText "graft-mismatched-manifest-input.json" (
    builtins.toJSON (
      updateManifestWorkload "nix-check-system" (
        workload: workload // { generatedService = "other.service"; }
      ) systemManifestInput.value
    )
  );
  failedManifestRender = name: input: pkgs.testers.testBuildFailure (renderManifestInput name input);
  incompleteManifestFailure = failedManifestRender "graft-incomplete-manifest-render" incompleteManifestInput;
  duplicateManifestFailure = failedManifestRender "graft-duplicate-manifest-render" duplicateManifestInput;
  mismatchedManifestFailure = failedManifestRender "graft-mismatched-manifest-render" mismatchedManifestInput;
  expectedSystemManifestSources = pkgs.writeText "graft-system-manifest-sources.json" (
    builtins.toJSON (builtins.attrNames systemManifestMaterialised.containers)
  );
  expectedUserManifestSources = pkgs.writeText "graft-user-manifest-sources.json" (
    builtins.toJSON (builtins.attrNames userManifestMaterialised.containers)
  );

  largeClosureMembers = lib.genList (
    index: builtins.toFile "graft-closure-member-${lib.fixedWidthString 3 "0" (toString index)}" ""
  ) 511;
  largeClosureRootfs = pkgs.runCommand "graft-large-closure-rootfs" { } ''
    mkdir -p $out/nix/store
    ${lib.concatMapStringsSep "\n" (member: ''
      touch "$out/nix/store/${builtins.baseNameOf member}"
      printf '%s\n' ${lib.escapeShellArg "${member}"} >> "$out/member-references"
    '') largeClosureMembers}
    mkdir "$out/nix/store/$(basename "$out")"
  '';
  largeClosureInfo = pkgs.closureInfo { rootPaths = [ largeClosureRootfs ]; };
  largeClosureTemplate = pkgs.writeText "graft-large-closure-template.container" ''
    [Container]
    ContainerName=closure-large
    Rootfs=${largeClosureRootfs}:O
    @GRAFT_STORE_MOUNTS@
    Exec="/bin/true"
    ReadOnly=true
  '';
  largeClosureSource = pkgs.runCommand "graft-large-closure.container" { } ''
    LC_ALL=C sort -u ${largeClosureInfo}/store-paths > actual-paths
    ${pkgs.bash}/bin/bash ${../../modules/lib/render-closure-mounts.sh} \
      actual-paths \
      ${largeClosureRootfs} \
      ${largeClosureTemplate} \
      "$out" \
      services.graft \
      closure-large \
      @GRAFT_STORE_MOUNTS@ \
      ${../../modules/lib/check-closure-limits.sh}
  '';
  closureTestPkgs = pkgs.extend (
    _: _: {
      graftClosureFixture = closureFixturePackage;
      graftRootfsEtcFixture = rootfsEtcFixturePackage;
    }
  );

  moduleTestOptions = { lib, ... }: {
    imports = [ ../../tests/nixos/lib/hm-test-options.nix ];

    options = {
      virtualisation.podman.enable = lib.mkOption {
        type = lib.types.bool;
        default = false;
      };

      environment.systemPackages = lib.mkOption {
        type = lib.types.listOf lib.types.package;
        default = [ ];
      };

      environment.etc = lib.mkOption {
        type = lib.types.attrsOf (
          lib.types.submodule (
            { config, ... }:
            {
              options = {
                source = lib.mkOption { type = lib.types.path; };
                text = lib.mkOption {
                  type = lib.types.str;
                  default = builtins.readFile config.source;
                };
              };
            }
          )
        );
        default = { };
      };

      users.groups = lib.mkOption {
        type = lib.types.attrsOf lib.types.anything;
        default = { };
      };

      system.build.graftManifestGeneration = lib.mkOption {
        type = lib.types.package;
      };

      system.activationScripts = lib.mkOption {
        type = lib.types.attrsOf (
          lib.types.submodule {
            options = {
              deps = lib.mkOption {
                type = lib.types.listOf lib.types.str;
                default = [ ];
              };
              text = lib.mkOption { type = lib.types.lines; };
              supportsDryActivation = lib.mkOption {
                type = lib.types.bool;
                default = false;
              };
            };
          }
        );
        default = { };
      };

      home.packages = lib.mkOption {
        type = lib.types.listOf lib.types.package;
        default = [ ];
      };

      systemd = {
        services = lib.mkOption {
          type = lib.types.attrsOf lib.types.anything;
          default = { };
        };
        sockets = lib.mkOption {
          type = lib.types.attrsOf lib.types.anything;
          default = { };
        };
        tmpfiles.rules = lib.mkOption {
          type = lib.types.listOf lib.types.str;
          default = [ ];
        };
      };
    };
  };

  nixosEval = lib.evalModules {
    specialArgs = { inherit pkgs; };
    modules = [
      moduleTestOptions
      self.nixosModules.graft
      {
        services.graft = {
          enable = true;
          hostId = manifestHostId;
          configRoot = ../../tests/nix/containers;
          configRoots = [ ../../tests/nix/containers-extra ];
        };
      }
    ];
  };

  evalHomeManager =
    {
      osConfig ? null,
      moduleArgsOsConfig ? null,
      hostId ? null,
    }:
    lib.evalModules {
      specialArgs = {
        inherit pkgs;
      }
      // lib.optionalAttrs (osConfig != null) { inherit osConfig; };
      modules = [
        moduleTestOptions
        self.homeManagerModules.graft
        (lib.optionalAttrs (moduleArgsOsConfig != null) {
          config._module.args.osConfig = moduleArgsOsConfig;
        })
        {
          programs.graft = {
            enable = true;
            configRoot = ../../tests/nix/containers;
            configRoots = [ ../../tests/nix/containers-extra ];
          }
          // lib.optionalAttrs (hostId != null) { inherit hostId; };
        }
      ];
    };
  homeManagerEval = evalHomeManager { };
  realHomeManagerEval = homeManager.lib.homeManagerConfiguration {
    inherit pkgs;
    modules = [
      self.homeManagerModules.graft
      {
        home = {
          username = "graft";
          homeDirectory = "/home/graft";
          stateVersion = "26.05";
        };
        programs.graft = {
          enable = true;
          package = graftPackage;
          hostId = manifestHostId;
          configRoot = ../../tests/nix/containers;
          configRoots = [ ../../tests/nix/containers-extra ];
        };
      }
    ];
  };
  homeManagerInheritedEval = evalHomeManager {
    osConfig.services.graft.hostId = manifestHostId;
  };
  homeManagerModuleArgsInheritedEval = evalHomeManager {
    moduleArgsOsConfig.services.graft.hostId = manifestHostId;
  };
  homeManagerMismatchEval = evalHomeManager {
    osConfig.services.graft.hostId = manifestHostId;
    hostId = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c21";
  };
  homeManagerHostIdInherited = homeManagerInheritedEval.config.programs.graft.hostId;
  homeManagerModuleArgsHostIdInherited =
    homeManagerModuleArgsInheritedEval.config.programs.graft.hostId;
  homeManagerHostIdMismatchRejected = lib.any (
    assertion: !assertion.assertion
  ) homeManagerMismatchEval.config.assertions;
  incompatibleWorkerApiRangeEval = lib.evalModules {
    specialArgs = { inherit pkgs; };
    modules = [
      moduleTestOptions
      self.nixosModules.graft
      {
        services.graft = {
          enable = true;
          hostId = manifestHostId;
          package = graftPackage // {
            graftWorkerApiRange = graftWorkerApiRange // {
              major = graftWorkerApiRange.major + 1;
            };
          };
        };
      }
    ];
  };
  incompatibleWorkerApiRangeRejected = lib.any (
    assertion:
    assertion.message == "services.graft.package has no compatible worker API range."
    && !assertion.assertion
  ) incompatibleWorkerApiRangeEval.config.assertions;

  networkNixosEval = lib.evalModules {
    specialArgs = { inherit pkgs; };
    modules = [
      moduleTestOptions
      self.nixosModules.graft
      {
        services.graft = {
          enable = true;
          hostId = manifestHostId;
          configRoot = ../../tests/nix/network;
        };
      }
    ];
  };

  networkHomeManagerEval = lib.evalModules {
    specialArgs = { inherit pkgs; };
    modules = [
      moduleTestOptions
      self.homeManagerModules.graft
      {
        programs.graft = {
          enable = true;
          configRoot = ../../tests/nix/network;
        };
      }
    ];
  };

  dependencyNixosEval = lib.evalModules {
    specialArgs = { inherit pkgs; };
    modules = [
      moduleTestOptions
      self.nixosModules.graft
      {
        services.graft = {
          enable = true;
          hostId = manifestHostId;
          configRoot = ../../tests/nix/dependencies;
        };
      }
    ];
  };

  dependencyHomeManagerEval = lib.evalModules {
    specialArgs = { inherit pkgs; };
    modules = [
      moduleTestOptions
      self.homeManagerModules.graft
      {
        programs.graft = {
          enable = true;
          configRoot = ../../tests/nix/dependencies;
        };
      }
    ];
  };

  cdiNixosEval = lib.evalModules {
    specialArgs = { inherit pkgs; };
    modules = [
      moduleTestOptions
      self.nixosModules.graft
      {
        services.graft = {
          enable = true;
          hostId = manifestHostId;
          configRoot = ../../tests/nix/cdi;
        };
      }
    ];
  };

  cdiHomeManagerEval = lib.evalModules {
    specialArgs = { inherit pkgs; };
    modules = [
      moduleTestOptions
      self.homeManagerModules.graft
      {
        programs.graft = {
          enable = true;
          configRoot = ../../tests/nix/cdi;
        };
      }
    ];
  };

  quickstartNixosEval = lib.evalModules {
    specialArgs = { inherit pkgs; };
    modules = [
      moduleTestOptions
      self.nixosModules.graft
      ../../examples/quickstart/nixos/module.nix
    ];
  };

  quickstartHomeManagerEval = lib.evalModules {
    specialArgs = { inherit pkgs; };
    modules = [
      moduleTestOptions
      self.homeManagerModules.graft
      ../../examples/quickstart/home-manager/module.nix
    ];
  };

  closureNixosEval = lib.evalModules {
    specialArgs.pkgs = closureTestPkgs;
    modules = [
      moduleTestOptions
      self.nixosModules.graft
      {
        services.graft = {
          enable = true;
          hostId = manifestHostId;
          configRoot = ../../tests/nix/closure;
        };
      }
    ];
  };

  closureHomeManagerEval = lib.evalModules {
    specialArgs.pkgs = closureTestPkgs;
    modules = [
      moduleTestOptions
      self.homeManagerModules.graft
      {
        programs.graft = {
          enable = true;
          configRoot = ../../tests/nix/closure;
        };
      }
    ];
  };

  closureNixosSource =
    closureNixosEval.config.environment.etc."containers/systemd/closure-system.container".source;
  closureHomeManagerSource =
    closureHomeManagerEval.config.xdg.configFile."containers/systemd/closure-user.container".source;
  closureNixosSourceClosure = pkgs.closureInfo { rootPaths = [ closureNixosSource ]; };
  closureHomeManagerSourceClosure = pkgs.closureInfo { rootPaths = [ closureHomeManagerSource ]; };

  nixosRendered =
    nixosEval.config.environment.etc."containers/systemd/nix-check-system.container".text;
  nixosPlainRendered =
    nixosEval.config.environment.etc."containers/systemd/nix-check-plain-system.container".text;
  nixosEscapeRendered =
    nixosEval.config.environment.etc."containers/systemd/escape-system.container".text;
  nixosHostRendered =
    nixosEval.config.environment.etc."containers/systemd/nix-check-host-system.container".text;
  nixosTimerJobRendered =
    nixosEval.config.environment.etc."containers/systemd/nix-check-timer-job-system.container".text;
  nixosStartupJobRendered =
    nixosEval.config.environment.etc."containers/systemd/nix-check-startup-job-system.container".text;
  nixosSetupRendered =
    nixosEval.config.environment.etc."containers/systemd/nix-check-setup-system.container".text;
  nixosNetworkOwnerRendered =
    networkNixosEval.config.environment.etc."containers/systemd/nix-check-network-owner-system.container".text;
  nixosNetworkClientRendered =
    networkNixosEval.config.environment.etc."containers/systemd/nix-check-network-client-system.container".text;
  nixosNetworkNoneRendered =
    networkNixosEval.config.environment.etc."containers/systemd/nix-check-network-none-system.container".text;
  homeManagerRendered =
    homeManagerEval.config.xdg.configFile."containers/systemd/nix-check-user.container".text;
  homeManagerPlainRendered =
    homeManagerEval.config.xdg.configFile."containers/systemd/nix-check-plain-user.container".text;
  homeManagerEscapeRendered =
    homeManagerEval.config.xdg.configFile."containers/systemd/escape-user.container".text;
  homeManagerHostRendered =
    homeManagerEval.config.xdg.configFile."containers/systemd/nix-check-host-user.container".text;
  homeManagerTimerJobRendered =
    homeManagerEval.config.xdg.configFile."containers/systemd/nix-check-timer-job-user.container".text;
  homeManagerStartupJobRendered =
    homeManagerEval.config.xdg.configFile."containers/systemd/nix-check-startup-job-user.container".text;
  homeManagerSetupRendered =
    homeManagerEval.config.xdg.configFile."containers/systemd/nix-check-setup-user.container".text;
  homeManagerNetworkOwnerRendered =
    networkHomeManagerEval.config.xdg.configFile."containers/systemd/nix-check-network-owner-user.container".text;
  homeManagerNetworkClientRendered =
    networkHomeManagerEval.config.xdg.configFile."containers/systemd/nix-check-network-client-user.container".text;
  homeManagerNetworkNoneRendered =
    networkHomeManagerEval.config.xdg.configFile."containers/systemd/nix-check-network-none-user.container".text;
  nixosDependencyOwnerRendered =
    dependencyNixosEval.config.environment.etc."containers/systemd/dependency-owner-system.container".text;
  nixosDependencyClientRendered =
    dependencyNixosEval.config.environment.etc."containers/systemd/dependency-client-system.container".text;
  homeManagerDependencyOwnerRendered =
    dependencyHomeManagerEval.config.xdg.configFile."containers/systemd/dependency-owner-user.container".text;
  homeManagerDependencyClientRendered =
    dependencyHomeManagerEval.config.xdg.configFile."containers/systemd/dependency-client-user.container".text;
  nixosCdiRendered =
    cdiNixosEval.config.environment.etc."containers/systemd/nix-check-cdi-system.container".text;
  homeManagerCdiRendered =
    cdiHomeManagerEval.config.xdg.configFile."containers/systemd/nix-check-cdi-user.container".text;
  quickstartNixosRendered =
    quickstartNixosEval.config.environment.etc."containers/systemd/graft-example.container".text;
  quickstartHomeManagerRendered =
    quickstartHomeManagerEval.config.xdg.configFile."containers/systemd/graft-example.container".text;
  expectedQuickstartInfixes = [
    "ContainerName=graft-example"
    ''Exec="bash" "-c" "echo graft-example-ready; exec /bin/graft-pause"''
    "/nix/store:/nix/store:ro,bind,nodev,nosuid"
    ":ro,bind,nodev,nosuid"
  ];
  expectedEnvironmentLines = lib.concatStringsSep "\n" [
    ''Environment="EMPTY="''
    ''Environment="EQUALS=a=b"''
    ''Environment="GREETING=hello world"''
    ''Environment="LOG_LEVEL=debug"''
    ''Environment="PATHLIKE=C:\\Temp"''
    ''Environment="PERCENT=100%%"''
    ''Environment="QUOTED=say \"hi\""''
  ];
  expectedEscapedEnvironmentLines = lib.concatStringsSep "\n" [
    "Environment=\"BRACED=pre$\${HOME}post\""
    "Environment=\"DOLLAR=cost $$5\""
    "Environment=\"PERCENT=100%%\""
  ];
  assertHasInfixes =
    content: infixes:
    lib.all (
      infix:
      lib.assertMsg (lib.hasInfix infix content) "expected rendered output to contain ${builtins.toJSON infix}"
    ) infixes;
  assertNoInfixes =
    content: infixes:
    lib.all (
      infix:
      lib.assertMsg (
        !(lib.hasInfix infix content)
      ) "expected rendered output not to contain ${builtins.toJSON infix}"
    ) infixes;
  commonRenderedInfixes = [
    "User=1000"
    "Group=1000"
    "WorkingDir=/workspace"
    expectedEnvironmentLines
    "\n[Service]\nType=notify\nRestart=on-failure\nRestartSec=10s\nTimeoutStartSec=2m\nTimeoutStopSec=30s"
  ];
  commonEscapedInfixes = [
    "User=100%%0"
    "Group=100%%0"
    "WorkingDir=/work%%space/$$HOME"
    "Exec=\"/bin/echo\" \"pre$\${HOME}post\" \"100%%\" \"cost $$5\" \"foo\\\\.bar\" \"C:\\\\Temp\" \"say \\\"hi\\\"\""
    expectedEscapedEnvironmentLines
    "EnvironmentFile=\"/etc/graft/$$USER-%%n.env\"\nEnvironmentFile=\"/etc/graft/my config.env\"\nEnvironmentFile=\"/etc/graft/env\\\\prod.env\""
    "Volume=/tmp/graft-$$USER-%%n:/data$$HOME-%%h:ro,bind"
    "\n[Service]\nRestart=on-failure\nRestartSec=15s"
  ];
  secureBaselineInfixes = [
    "ReadOnly=true"
    "DropCapability=all"
    "NoNewPrivileges=true"
  ];
  commonPlainMissingInfixes = [
    "HostName="
    "User="
    "Group="
    "WorkingDir="
    "Environment="
    "EnvironmentFile="
    "PublishPort="
    "Network="
    "AddCapability="
    "Type="
    "RemainAfterExit="
    "Restart="
    "RestartSec="
    "TimeoutStartSec="
    "TimeoutStopSec="
    "WantedBy="
    "[Unit]"
    "Requires="
    "Wants="
    "After="
    "Before="
    "PartOf="
    "BindsTo="
  ];
  renderAssertions =
    {
      rendered,
      plainRendered,
      escapeRendered,
      renderedInfixes,
      escapeInfixes,
      plainMissingInfixes,
    }:
    assertHasInfixes rendered (secureBaselineInfixes ++ commonRenderedInfixes ++ renderedInfixes)
    && assertHasInfixes escapeRendered (secureBaselineInfixes ++ commonEscapedInfixes ++ escapeInfixes)
    && assertHasInfixes plainRendered secureBaselineInfixes
    && assertNoInfixes plainRendered (commonPlainMissingInfixes ++ plainMissingInfixes);
  evalNixosWithRoots =
    extraRoots:
    lib.evalModules {
      specialArgs = { inherit pkgs; };
      modules = [
        moduleTestOptions
        self.nixosModules.graft
        {
          services.graft = {
            enable = true;
            hostId = manifestHostId;
            configRoot = ../../tests/nix/containers;
            configRoots = extraRoots;
          };
        }
      ];
    };
  evalSystemPublication =
    settings:
    lib.evalModules {
      specialArgs = { inherit pkgs; };
      modules = [
        moduleTestOptions
        self.nixosModules.graft
        {
          services.graft = {
            enable = true;
          }
          // settings;
        }
      ];
    };
  missingSystemHostIdEval = evalSystemPublication { };
  missingSystemHostIdRejected = lib.any (
    assertion: !assertion.assertion
  ) missingSystemHostIdEval.config.assertions;
  missingSystemPackageEval = lib.evalModules {
    specialArgs = { inherit pkgs; };
    modules = [
      moduleTestOptions
      ../../modules/nixos.nix
      {
        services.graft = {
          enable = true;
          hostId = manifestHostId;
        };
      }
    ];
  };
  missingSystemPackageRejected = lib.any (
    assertion: !assertion.assertion
  ) missingSystemPackageEval.config.assertions;
  invalidSystemHostIds = [
    ""
    "018F0F77-8C4D-7B2A-8E6A-4B8A7D3A1C20"
    "{018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20}"
    "018f0f778c4d7b2a8e6a4b8a7d3a1c20"
    "g18f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20"
    "018f0f77-8c4d-6b2a-8e6a-4b8a7d3a1c20"
    "018f0f77-8c4d-7b2a-0e6a-4b8a7d3a1c20"
    "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20-extra"
  ];
  invalidSystemHostIdsRejected = lib.all (
    hostId:
    !(builtins.tryEval (
      builtins.deepSeq (evalSystemPublication { inherit hostId; }).config.services.graft.hostId true
    )).success
  ) invalidSystemHostIds;
  systemPublicationGeneration = nixosEval.config.system.build.graftManifestGeneration;
  systemPublicationActivation = nixosEval.config.system.activationScripts.graftManifestPublication;
  systemWorkerService = nixosEval.config.systemd.services.graft-system-worker;
  systemWorkerSocket = nixosEval.config.systemd.sockets.graft-system-worker;
  expectedSystemPublicationSources = pkgs.writeText "graft-system-publication-sources.json" (
    builtins.toJSON (
      map (path: "${lib.removeSuffix ".container" (builtins.baseNameOf path)}.toml") (
        builtins.attrNames nixosEval.config.environment.etc
      )
    )
  );

  evalHomeManagerWithRoots =
    extraRoots:
    lib.evalModules {
      specialArgs = { inherit pkgs; };
      modules = [
        moduleTestOptions
        self.homeManagerModules.graft
        {
          programs.graft = {
            enable = true;
            configRoot = ../../tests/nix/containers;
            configRoots = extraRoots;
          };
        }
      ];
    };
  duplicateFilenameNixosEval = evalNixosWithRoots [ ../../tests/nix/duplicate-filename ];
  duplicateFilenameHomeManagerEval = evalHomeManagerWithRoots [ ../../tests/nix/duplicate-filename ];
  unsafeSourceStemsNixosEval = evalNixosWithRoots [ ../../tests/nix/unsafe-source-stems ];
  unsafeSourceStemsHomeManagerEval = evalHomeManagerWithRoots [ ../../tests/nix/unsafe-source-stems ];
  duplicateFilenameNixosFails =
    !(builtins.tryEval (builtins.deepSeq duplicateFilenameNixosEval.config.environment.etc true))
    .success;
  duplicateFilenameHomeManagerFails =
    !(builtins.tryEval (builtins.deepSeq duplicateFilenameHomeManagerEval.config.xdg.configFile true))
    .success;
  unsafeSourceStemsNixosFails =
    !(builtins.tryEval (builtins.deepSeq unsafeSourceStemsNixosEval.config.environment.etc true))
    .success;
  unsafeSourceStemsHomeManagerFails =
    !(builtins.tryEval (builtins.deepSeq unsafeSourceStemsHomeManagerEval.config.xdg.configFile true))
    .success;

  # Shared mechanical harness for Quadlet checks. Feature checks keep their
  # source declarations and assertions next to the check that owns them.
  quadletTestHelper =
    { sources }:
    let
      installSources = lib.concatStringsSep "\n" (
        lib.mapAttrsToList (name: source: "cp ${source} source-system/${name}.container") sources.system
        ++ lib.mapAttrsToList (name: source: "cp ${source} source-user/${name}.container") sources.user
      );
      generate =
        scope: expectFailure:
        let
          userFlag = lib.optionalString (scope == "user") " -user";
          generated = "generated-${scope}";
          error = "${scope}-generator-error";
          command = "${pkgs.podman}/libexec/podman/quadlet${userFlag} ${generated} ${generated} ${generated}";
        in
        if expectFailure then
          ''
            if QUADLET_UNIT_DIRS="$PWD/source-${scope}" ${command} 2> ${error}; then
              echo "unexpectedly accepted malformed Quadlet source" >&2
              exit 1
            fi
          ''
        else
          ''
            QUADLET_UNIT_DIRS="$PWD/source-${scope}" ${command} 2> ${error}
            test ! -s ${error}
          '';
      verify = ''
        mkdir -p runtime/systemd
        XDG_RUNTIME_DIR="$PWD/runtime" \
          SYSTEMD_UNIT_PATH="$PWD/generated-system:$PWD/generated-user:${pkgs.podman}/share/systemd/user:${pkgs.systemd}/example/systemd/user:${pkgs.systemd}/example/systemd/system" \
          ${lib.getExe' pkgs.systemd "systemd-analyze"} --user verify \
          generated-system/*.service generated-user/*.service
      '';
    in
    {
      setup = ''
        mkdir source-system source-user generated-system generated-user $out
        ${installSources}
      '';
      inherit generate verify;
    };
in
{
  version-parity =
    assert !(builtins.tryEval (requireVersionParity graftVersion "deliberate-drift")).success;
    pkgs.runCommand "graft-version-parity" { } ''
      set -euo pipefail
      test ${lib.escapeShellArg graftPackage.version} = ${lib.escapeShellArg graftVersion}
      test "$(${lib.getExe graftPackage} --version)" = ${lib.escapeShellArg "graft ${graftVersion}"}
      test "$(${lib.getExe' graftPackage "graft-worker"} --version)" = ${lib.escapeShellArg "graft-worker ${graftVersion}"}
      touch "$out"
    '';

  build-id-fallback =
    assert buildIdForRevision null == "source";
    assert buildIdForRevision "deliberate-revision" == "deliberate-revision";
    pkgs.runCommand "graft-build-id-fallback" { } ''
      test ${lib.escapeShellArg manifestBuildId} = ${lib.escapeShellArg graftBuildId}
      touch "$out"
    '';

  manifest-lowering =
    pkgs.runCommand "graft-manifest-lowering"
      {
        nativeBuildInputs = [ pkgs.jq ];
      }
      ''
        set -euo pipefail

        check_context() {
          input=$1
          documents=$2
          target=$3
          expected_sources=$4

          test -f "$documents/manifest.json"
          test -f "$documents/endpoint.json"
          test "$(find "$documents" -mindepth 1 -maxdepth 1 -type f | wc -l)" -eq 2

          jq -S '.workloads' "$input" > input-workloads.json
          jq -S '.workloads' "$documents/manifest.json" > manifest-workloads.json
          cmp input-workloads.json manifest-workloads.json

          jq -S '.' "$expected_sources" > expected-sources.json
          jq -S '[.workloads[].sourceIdentity]' "$input" > actual-sources.json
          cmp expected-sources.json actual-sources.json
          ! grep -Eq '"(sourcePath|quadletSource|closureInfo|resolved|environment|command|secret|credential)"[[:space:]]*:' "$input"

          jq -e --arg target "$target" '
            .target == $target
            and .manager == $target
            and (.workloads as $workloads
              | [$workloads[].name] == ([$workloads[].name] | sort)
              and all($workloads[];
                .enabled == true
                and .target == $target
                and .sourceIdentity == (.name + ".toml")
                and .quadletSourceUnit == (.name + ".container")
                and .generatedService == (.name + ".service")
                and .containerName == .name
                and .lifecycleCapabilities == []
                and .observabilityCapabilities == ["manifest"]
                and (.rootfsStorePath | startswith("/nix/store/"))))
          ' "$input"
          jq -e --arg target "$target" '
            .target == $target
            and .manager == $target
            and .generationId == .manifestDigest
            and (.workloads | length) == .workloadCount
            and (has("uid") | not)
          ' "$documents/manifest.json"
          jq -e '
            .generationId == .manifestDigest
            and (.socketAddress | has("kind"))
            and (has("uid") | not)
          ' "$documents/endpoint.json"
        }

        check_context \
          ${systemManifestInput.file} \
          ${systemManifestDocuments} \
          system \
          ${expectedSystemManifestSources}
        check_context \
          ${userManifestInput.file} \
          ${userManifestDocuments} \
          user \
          ${expectedUserManifestSources}

        jq -e '
          ([.workloads[].lifecycle] | unique) == ["job", "long_running", "setup"]
          and ([.workloads[].startupIntent] | unique) == ["disabled", "manager_target"]
          and any(.workloads[];
            .name == "dependency-client-system"
            and (.dependencyServices | index("dependency-owner-system.service")))
        ' ${systemManifestInput.file}

        touch "$out"
      '';

  manifest-lowering-rejects-incomplete = incompleteManifestFailure;
  manifest-lowering-rejects-duplicate = duplicateManifestFailure;
  manifest-lowering-rejects-mismatch = mismatchedManifestFailure;

  system-manifest-publication =
    assert missingSystemHostIdRejected;
    assert missingSystemPackageRejected;
    assert invalidSystemHostIdsRejected;
    assert nixosEval.config.users.groups ? graft;
    assert lib.all (dependency: lib.elem dependency systemPublicationActivation.deps) [
      "users"
      "groups"
      "etc"
    ];
    assert lib.hasInfix "graft-manifest-publish-system" (
      builtins.unsafeDiscardStringContext systemPublicationActivation.text
    );
    assert lib.hasInfix (builtins.unsafeDiscardStringContext (
      builtins.baseNameOf systemPublicationGeneration
    )) (builtins.unsafeDiscardStringContext systemPublicationActivation.text);
    assert !lib.hasInfix "XDG_" systemPublicationActivation.text;
    assert !lib.hasInfix "/home/" systemPublicationActivation.text;
    assert systemWorkerSocket.socketConfig.ListenStream == "/run/graft/system/worker.sock";
    assert systemWorkerSocket.socketConfig.SocketMode == "0660";
    assert systemWorkerSocket.socketConfig.SocketGroup == "graft";
    assert systemWorkerService.serviceConfig.User == "root";
    assert systemWorkerService.serviceConfig.Restart == "on-failure";
    assert lib.hasInfix "--target system --effective-uid 0 --manager system"
      systemWorkerService.serviceConfig.ExecStart;
    assert lib.hasInfix "--graft-gid" systemWorkerService.serviceConfig.ExecStart;
    pkgs.runCommand "graft-system-manifest-publication"
      {
        nativeBuildInputs = [ pkgs.jq ];
      }
      ''
        set -euo pipefail
        generation=${systemPublicationGeneration}

        test "$(stat -c '%a:%F' "$generation")" = "555:directory"
        test "$(stat -c '%a:%F' "$generation/manifest.json")" = "444:regular file"
        test "$(stat -c '%a:%F' "$generation/endpoint.json")" = "444:regular file"
        test "$(find "$generation" -mindepth 1 -maxdepth 1 -printf '%f\n' | LC_ALL=C sort)" = $'endpoint.json\nmanifest.json'

        jq -e --arg host ${lib.escapeShellArg manifestHostId} --arg version ${lib.escapeShellArg graftVersion} --arg buildId ${lib.escapeShellArg manifestBuildId} '
          .hostId == $host
          and .target == "system"
          and .manager == "system"
          and .producer == {
            name: "graft",
            version: $version,
            buildId: $buildId
          }
          and .generationId == .manifestDigest
          and (.workloads | length) == .workloadCount
          and (has("uid") | not)
        ' "$generation/manifest.json"
        jq -e --arg host ${lib.escapeShellArg manifestHostId} --arg version ${lib.escapeShellArg graftVersion} --arg buildId ${lib.escapeShellArg manifestBuildId} '
          .hostId == $host
          and .target == "system"
          and .manager == "system"
          and .producer == {
            name: "graft",
            version: $version,
            buildId: $buildId
          }
          and .generationId == .manifestDigest
          and (has("uid") | not)
        ' "$generation/endpoint.json"

        manifest_generation=$(jq -r '.generationId' "$generation/manifest.json")
        endpoint_generation=$(jq -r '.generationId' "$generation/endpoint.json")
        endpoint_manifest=$(jq -r '.manifestDigest' "$generation/endpoint.json")
        test -n "$manifest_generation"
        test "$endpoint_generation" = "$manifest_generation"
        test "$endpoint_manifest" = "$manifest_generation"

        jq -S '.' ${expectedSystemPublicationSources} > expected-sources.json
        jq -S '[.workloads[].sourceIdentity]' "$generation/manifest.json" > actual-sources.json
        cmp expected-sources.json actual-sources.json
        ! grep -Eqi '"(environment|command|secret|credential|uid)"[[:space:]]*:' "$generation/manifest.json" "$generation/endpoint.json"

        touch "$out"
      '';

  nixos-module-eval =
    assert incompatibleWorkerApiRangeRejected;
    assert renderAssertions {
      rendered = nixosRendered;
      plainRendered = nixosPlainRendered;
      escapeRendered = nixosEscapeRendered;
      renderedInfixes = [
        "ContainerName=nix-check-system"
        "HostName=nix-check-system.local"
        "EnvironmentFile=\"/etc/graft/system.env\"\nEnvironmentFile=\"/run/graft/shared.env\""
        "Tmpfs=/run/graft-system:rw,noexec,nosuid,nodev,mode=0750,size=64M\nTmpfs=/tmp/graft-system:rw,noexec,nosuid,nodev"
        "Volume=/tmp/graft-system-data:/data:rw,bind\nVolume=/tmp/graft-system-config:/config:ro,bind\nVolume=/system-cache"
        "PublishPort=127.0.0.1:18080:80\nPublishPort=18443:443/tcp"
        "\n[Install]\nWantedBy=multi-user.target"
      ];
      escapeInfixes = [
        "ContainerName=escape-system"
        "HostName=escape%%system.local"
        "PublishPort=127.0.0.1:18%%080:80"
      ];
      plainMissingInfixes = [
        "Volume=/system-cache"
        "Volume=/tmp/graft-system-data:/data:rw,bind"
        "Volume=/tmp/graft-system-config:/config:ro,bind"
      ];
    };
    assert assertHasInfixes nixosHostRendered [
      "ContainerName=nix-check-host-system"
      "HostName=host-system.local"
    ];
    assert assertHasInfixes nixosTimerJobRendered [
      "ContainerName=nix-check-timer-job-system"
      ''Exec="/bin/true"''
      "\n[Service]\nType=oneshot\nRemainAfterExit=no\nRestart=on-failure\nRestartSec=10s\nTimeoutStartSec=2m\nTimeoutStopSec=30s"
    ];
    assert assertNoInfixes nixosTimerJobRendered [ "WantedBy=" ];
    assert assertHasInfixes nixosStartupJobRendered [
      "ContainerName=nix-check-startup-job-system"
      ''Exec="/bin/true"''
      "\n[Service]\nType=oneshot\nRemainAfterExit=no"
      "\n[Install]\nWantedBy=multi-user.target"
    ];
    assert assertHasInfixes nixosSetupRendered [
      "ContainerName=nix-check-setup-system"
      ''Exec="/bin/true"''
      "\n[Service]\nType=oneshot\nRemainAfterExit=yes\nTimeoutStartSec=2m\nTimeoutStopSec=30s"
      "\n[Install]\nWantedBy=multi-user.target"
    ];
    assert assertHasInfixes nixosNetworkOwnerRendered [
      "ContainerName=nix-check-network-owner-system"
    ];
    assert assertHasInfixes nixosNetworkClientRendered [
      "ContainerName=nix-check-network-client-system"
      "Network=nix-check-network-owner-system.container"
      "\n[Install]\nWantedBy=multi-user.target"
    ];
    assert assertNoInfixes nixosNetworkOwnerRendered [ "WantedBy=" ];
    assert assertHasInfixes nixosNetworkNoneRendered [
      "ContainerName=nix-check-network-none-system"
      "Network=none"
    ];
    assert assertHasInfixes nixosDependencyClientRendered [
      "[Unit]\nRequires=dependency-owner-system.container\nWants=graft-foreign-system.service\nAfter=dependency-owner-system.container\nBefore=graft-foreign-system.service\nPartOf=dependency-owner-system.container\nBindsTo=graft-bound-system.service"
      "ContainerName=dependency-client-system"
      "\n[Install]\nWantedBy=multi-user.target"
    ];
    assert assertNoInfixes nixosDependencyOwnerRendered [
      "[Unit]"
      "WantedBy="
    ];
    assert
      !(
        dependencyNixosEval.config.environment.etc ? "containers/systemd/dependency-client-user.container"
      );
    assert !(nixosEval.config.environment.etc ? "containers/systemd/nix-check-user.container");
    assert !(nixosEval.config.environment.etc ? "containers/systemd/escape-user.container");
    assert !(nixosEval.config.environment.etc ? "containers/systemd/nix-check-host-user.container");
    assert
      !(
        nixosEval.config.environment.etc ? "containers/systemd/nix-check-disabled-startup-system.container"
      );
    assert
      !(
        nixosEval.config.environment.etc ? "containers/systemd/nix-check-disabled-startup-user.container"
      );
    assert duplicateFilenameNixosFails;
    assert unsafeSourceStemsNixosFails;
    assert quickstartNixosEval.config.virtualisation.podman.enable;
    assert assertHasInfixes quickstartNixosRendered (
      expectedQuickstartInfixes ++ [ ''Environment="GRAFT_EXAMPLE=nixos-system"'' ]
    );
    assert !(lib.hasInfix "WorkingDir=" quickstartNixosRendered);
    pkgs.writeText "graft-nixos-module-eval" nixosRendered;

  home-manager-real-module-eval =
    let
      assertions = realHomeManagerEval.config.assertions;
      workerExecStart = lib.attrByPath [
        "systemd"
        "user"
        "services"
        "graft-user-worker"
        "Service"
        "ExecStart"
      ] null realHomeManagerEval.config;
      manifestPublication = lib.attrByPath [
        "home"
        "activation"
        "graftManifestPublication"
      ] null realHomeManagerEval.config;
      manifestPublicationText = lib.attrByPath [ "data" ] null manifestPublication;
    in
    assert builtins.deepSeq assertions (lib.all (assertion: assertion.assertion) assertions);
    assert realHomeManagerEval.config.programs.graft.enable;
    assert realHomeManagerEval.config.programs.graft.package == graftPackage;
    assert realHomeManagerEval.config.programs.graft.hostId == manifestHostId;
    assert
      realHomeManagerEval.config.systemd.user.services.graft-user-worker.Service.ProtectHome
      == "read-only";
    assert
      realHomeManagerEval.config.systemd.user.sockets.graft-user-worker.Install.WantedBy
      == [ "sockets.target" ];
    assert builtins.isList workerExecStart;
    assert builtins.length workerExecStart == 1;
    assert builtins.isString (builtins.elemAt workerExecStart 0);
    assert builtins.elemAt workerExecStart 0 != "";
    assert lib.hasInfix "graft-worker --target user --effective-uid %U --manager user" (
      builtins.elemAt workerExecStart 0
    );
    assert builtins.isAttrs manifestPublication;
    assert builtins.isString manifestPublicationText;
    assert manifestPublicationText != "";
    assert lib.hasInfix "graft-manifest-publish-user" manifestPublicationText;
    assert lib.hasInfix "--config-home /home/graft/.config" manifestPublicationText;
    assert lib.hasInfix "--state-home /home/graft/.local/state" manifestPublicationText;
    assert lib.hasInfix "--producer-name graft" manifestPublicationText;
    assert realHomeManagerEval.config.xdg.configFile ? "containers/systemd/nix-check-user.container";
    pkgs.writeText "graft-home-manager-real-module-eval" "real Home Manager evaluation succeeded";

  home-manager-module-eval =
    assert homeManagerEval.config.programs.graft.hostId == null;
    assert homeManagerHostIdInherited == manifestHostId;
    assert homeManagerModuleArgsHostIdInherited == manifestHostId;
    assert homeManagerHostIdMismatchRejected;
    assert
      homeManagerEval.config.systemd.user.services.graft-user-worker.Unit.Description
      == "Graft user worker";
    assert
      homeManagerEval.config.systemd.user.services.graft-user-worker.Service.Restart == "on-failure";
    assert
      homeManagerEval.config.systemd.user.sockets.graft-user-worker.Socket.ListenStream
      == "%t/graft/user/worker.sock";
    assert
      homeManagerEval.config.systemd.user.sockets.graft-user-worker.Install.WantedBy
      == [ "sockets.target" ];
    assert lib.hasInfix "$DRY_RUN_CMD" (
      builtins.unsafeDiscardStringContext homeManagerInheritedEval.config.home.activation.graftManifestPublication
    );
    assert renderAssertions {
      rendered = homeManagerRendered;
      plainRendered = homeManagerPlainRendered;
      escapeRendered = homeManagerEscapeRendered;
      renderedInfixes = [
        "ContainerName=nix-check-user"
        "HostName=nix-check-user.local"
        "EnvironmentFile=\"/etc/graft/user.env\"\nEnvironmentFile=\"/run/graft/shared.env\""
        "Tmpfs=/run/graft-user:rw,noexec,nosuid,nodev,mode=0750,size=64M\nTmpfs=/tmp/graft-user:rw,noexec,nosuid,nodev"
        "Volume=/tmp/graft-user-data:/data:rw,bind\nVolume=/tmp/graft-user-config:/config:ro,bind\nVolume=/user-cache"
        "PublishPort=127.0.0.1:28080:80\nPublishPort=28443:443/tcp"
        "\n[Install]\nWantedBy=default.target"
      ];
      escapeInfixes = [
        "ContainerName=escape-user"
        "HostName=escape%%user.local"
        "PublishPort=127.0.0.1:28%%080:80"
      ];
      plainMissingInfixes = [
        "Volume=/user-cache"
        "Volume=/tmp/graft-user-data:/data:rw,bind"
        "Volume=/tmp/graft-user-config:/config:ro,bind"
      ];
    };
    assert assertHasInfixes homeManagerHostRendered [
      "ContainerName=nix-check-host-user"
      "HostName=host-user.local"
    ];
    assert assertHasInfixes homeManagerTimerJobRendered [
      "ContainerName=nix-check-timer-job-user"
      ''Exec="/bin/true"''
      "\n[Service]\nType=oneshot\nRemainAfterExit=no\nRestart=on-failure\nRestartSec=10s\nTimeoutStartSec=2m\nTimeoutStopSec=30s"
    ];
    assert assertNoInfixes homeManagerTimerJobRendered [ "WantedBy=" ];
    assert assertHasInfixes homeManagerStartupJobRendered [
      "ContainerName=nix-check-startup-job-user"
      ''Exec="/bin/true"''
      "\n[Service]\nType=oneshot\nRemainAfterExit=no"
      "\n[Install]\nWantedBy=default.target"
    ];
    assert assertHasInfixes homeManagerSetupRendered [
      "ContainerName=nix-check-setup-user"
      ''Exec="/bin/true"''
      "\n[Service]\nType=oneshot\nRemainAfterExit=yes\nTimeoutStartSec=2m\nTimeoutStopSec=30s"
      "\n[Install]\nWantedBy=default.target"
    ];
    assert assertHasInfixes homeManagerNetworkOwnerRendered [
      "ContainerName=nix-check-network-owner-user"
    ];
    assert assertHasInfixes homeManagerNetworkClientRendered [
      "ContainerName=nix-check-network-client-user"
      "Network=nix-check-network-owner-user.container"
      "\n[Install]\nWantedBy=default.target"
    ];
    assert assertNoInfixes homeManagerNetworkOwnerRendered [ "WantedBy=" ];
    assert assertHasInfixes homeManagerNetworkNoneRendered [
      "ContainerName=nix-check-network-none-user"
      "Network=none"
    ];
    assert assertHasInfixes homeManagerDependencyClientRendered [
      "[Unit]\nRequires=dependency-owner-user.container\nWants=graft-foreign-user.service\nAfter=dependency-owner-user.container\nBefore=graft-foreign-user.service\nPartOf=dependency-owner-user.container\nBindsTo=graft-bound-user.service"
      "ContainerName=dependency-client-user"
      "\n[Install]\nWantedBy=default.target"
    ];
    assert assertNoInfixes homeManagerDependencyOwnerRendered [
      "[Unit]"
      "WantedBy="
    ];
    assert
      !(
        dependencyHomeManagerEval.config.xdg.configFile
        ? "containers/systemd/dependency-client-system.container"
      );
    assert !(homeManagerEval.config.xdg.configFile ? "containers/systemd/nix-check-system.container");
    assert !(homeManagerEval.config.xdg.configFile ? "containers/systemd/escape-system.container");
    assert
      !(homeManagerEval.config.xdg.configFile ? "containers/systemd/nix-check-host-system.container");
    assert
      !(
        homeManagerEval.config.xdg.configFile
        ? "containers/systemd/nix-check-disabled-startup-system.container"
      );
    assert
      !(
        homeManagerEval.config.xdg.configFile
        ? "containers/systemd/nix-check-disabled-startup-user.container"
      );
    assert duplicateFilenameHomeManagerFails;
    assert unsafeSourceStemsHomeManagerFails;
    assert assertHasInfixes quickstartHomeManagerRendered (
      expectedQuickstartInfixes ++ [ ''Environment="GRAFT_EXAMPLE=home-manager-user"'' ]
    );
    assert !(lib.hasInfix "WorkingDir=" quickstartHomeManagerRendered);
    pkgs.writeText "graft-home-manager-module-eval" homeManagerRendered;

  # Both module materialisers use this resolver boundary. Keep mismatch
  # failures in a derivation because a failing IFD cannot be caught by
  # a module-evaluation assertion.
  identity-mismatch = pkgs.runCommand "graft-identity-mismatch" { } ''
    set -euo pipefail

    for source in ${../../tests/nix/identity-mismatch}/*.toml; do
      if ${lib.getExe' graftPackage "graft"} "$source" > resolved.json 2> error; then
        echo "mismatched TOML stem and name resolved successfully: $source" >&2
        exit 1
      fi
      test ! -s resolved.json
      grep -F "canonical workload identity mismatch" error
      grep -F "TOML filename stem" error
      grep -F "configured name" error
    done

    touch "$out"
  '';

  rootfs-materialisation = pkgs.runCommand "graft-rootfs-materialisation" { } ''
    set -euo pipefail

    prepare_rootfs() {
      rootfs=$1
      mkdir -p "$rootfs/etc" "$rootfs/run"
      ln -s /proc/mounts "$rootfs/etc/mtab"
      touch \
        "$rootfs/etc/hostname" \
        "$rootfs/etc/hosts" \
        "$rootfs/etc/resolv.conf" \
        "$rootfs/run/.containerenv"
    }

    mkdir inner-empty rootfs-empty
    prepare_rootfs rootfs-empty
    ${pkgs.bash}/bin/bash ${../../modules/lib/materialise-rootfs-etc.sh} \
      inner-empty rootfs-empty services.graft no-etc

    mkdir rootfs-missing-mtab
    prepare_rootfs rootfs-missing-mtab
    rm rootfs-missing-mtab/etc/mtab
    if ${pkgs.bash}/bin/bash ${../../modules/lib/materialise-rootfs-etc.sh} \
      inner-empty rootfs-missing-mtab services.graft missing-mtab \
      2> missing-mtab-error; then
      echo "missing Graft-owned '/etc/mtab' was accepted" >&2
      exit 1
    fi
    grep -Fq "Graft-owned '/etc/mtab' is invalid" missing-mtab-error

    mkdir -p inner-valid/etc/graft-test
    echo materialised > inner-valid/etc/graft-test/config
    mkdir rootfs-valid
    prepare_rootfs rootfs-valid
    ${pkgs.bash}/bin/bash ${../../modules/lib/materialise-rootfs-etc.sh} \
      inner-valid rootfs-valid services.graft valid-etc
    grep -Fx materialised rootfs-valid/etc/graft-test/config

    for reserved_name in mtab hostname hosts resolv.conf; do
      mkdir -p "inner-reserved-$reserved_name/etc"
      touch "inner-reserved-$reserved_name/etc/$reserved_name"
      mkdir "rootfs-reserved-$reserved_name"
      prepare_rootfs "rootfs-reserved-$reserved_name"
      if ${pkgs.bash}/bin/bash ${../../modules/lib/materialise-rootfs-etc.sh} \
        "inner-reserved-$reserved_name" \
        "rootfs-reserved-$reserved_name" \
        services.graft \
        reserved-etc \
        2> "reserved-$reserved_name-error"; then
        echo "reserved package '/etc/$reserved_name' entry was accepted" >&2
        exit 1
      fi
      grep -Fq \
        "package content conflicts with Graft-owned '/etc/$reserved_name'" \
        "reserved-$reserved_name-error"
    done

    mkdir -p inner-dangling/etc/graft-test
    ln -s missing inner-dangling/etc/graft-test/dangling
    mkdir rootfs-dangling
    prepare_rootfs rootfs-dangling
    if ${pkgs.bash}/bin/bash ${../../modules/lib/materialise-rootfs-etc.sh} \
      inner-dangling rootfs-dangling services.graft dangling-etc \
      2> dangling-error; then
      echo "dangling package /etc entry was accepted" >&2
      exit 1
    fi
    grep -F "failed to materialise package /etc" dangling-error

    grep -F "two given paths contain a conflicting subpath" ${collisionFailure}/testBuildFailure.log

    mkdir "$out"
  '';

  closure-scoped-store =
    pkgs.runCommand "graft-closure-scoped-store"
      {
        nativeBuildInputs = [
          pkgs.coreutils
          pkgs.gnugrep
          pkgs.podman
          pkgs.systemd
        ];
      }
      ''
        set -euo pipefail
        mkdir "$out"

        check_source() {
          source_file=$1
          source_closure=$2
          expected_name=$3

          grep -Fx "ContainerName=$expected_name" "$source_file"
          ! grep -Fx 'Volume=/nix/store:/nix/store:ro' "$source_file"

          volume_lines=$(mktemp)
          grep '^Volume=' "$source_file" > "$volume_lines"
          first_volume=$(head -n 1 "$volume_lines")
          case "$first_volume" in
            Volume=*/nix/store:/nix/store:ro,bind,nodev,nosuid) ;;
            *) echo "missing first read-only store scaffold in $source_file" >&2; exit 1 ;;
          esac

          tail -n +2 "$volume_lines" | cut -d: -f1 | sed 's/^Volume=//' | LC_ALL=C sort -cu
          member_count=$(($(wc -l < "$volume_lines") - 1))
          test "$member_count" -gt 0
          test "$member_count" -le 512

          regular_line="Volume=${closureRegularFile}:${closureRegularFile}:ro,bind,nodev,nosuid"
          grep -Fx "$regular_line" "$source_file"
          grep -Fx ${lib.escapeShellArg "${closureRegularFile}"} "$source_closure/store-paths"

          scaffold_path="''${first_volume#Volume=}"
          scaffold_path="''${scaffold_path%:/nix/store:ro,bind,nodev,nosuid}"
          rootfs_path="''${scaffold_path%/nix/store}"
          test -d "$scaffold_path/$(basename "$rootfs_path")"
          test -f "$scaffold_path/$(basename ${lib.escapeShellArg "${closureRegularFile}"})"
          grep -Fx materialised "$rootfs_path/etc/graft-test/config"
          test -L "$rootfs_path/etc/mtab"
          test "$(readlink "$rootfs_path/etc/mtab")" = /proc/mounts
          test -f "$rootfs_path/etc/hostname"
          test -f "$rootfs_path/etc/hosts"
          test -f "$rootfs_path/etc/resolv.conf"
          test -f "$rootfs_path/run/.containerenv"

          cp "$source_file" "$out/$expected_name.container"
        }

        check_source ${closureNixosSource} ${closureNixosSourceClosure} closure-system
        check_source ${closureHomeManagerSource} ${closureHomeManagerSourceClosure} closure-user

        mkdir -p symlink-rootfs/nix/store
        printf '%s\n' ${closureSymlink} > symlink-paths
        if ${pkgs.bash}/bin/bash ${../../modules/lib/prepare-closure-targets.sh} \
          symlink-paths symlink-rootfs services.graft symlink-test \
          2> symlink-error; then
          echo "top-level closure symlink was accepted" >&2
          exit 1
        fi
        grep -F "top-level closure symlink '${closureSymlink}' is unsupported" symlink-error

        printf '/nix/store/expected\n' > expected-paths
        printf '/nix/store/expected\n/nix/store/unexpected\n' > mismatched-paths
        if ${pkgs.bash}/bin/bash ${../../modules/lib/check-closure-equality.sh} \
          expected-paths mismatched-paths services.graft mismatch-test \
          2> mismatch-error; then
          echo "mismatched final closure was accepted" >&2
          exit 1
        fi
        grep -F "final closure mismatch for container 'mismatch-test'" mismatch-error
        grep -F '+/nix/store/unexpected' mismatch-error

        seq 512 | sed 's#^#/nix/store/member-#' > exact-members
        truncate -s 131072 exact-fragment
        ${pkgs.bash}/bin/bash ${../../modules/lib/check-closure-limits.sh} \
          exact-members exact-fragment services.graft exact-limit

        cp exact-members too-many-members
        printf '/nix/store/member-513\n' >> too-many-members
        if ${pkgs.bash}/bin/bash ${../../modules/lib/check-closure-limits.sh} \
          too-many-members exact-fragment services.graft too-many \
          2> member-limit-error; then
          echo "513-member closure was accepted" >&2
          exit 1
        fi
        grep -F "has 513 members; limit is 512" member-limit-error

        truncate -s 131073 oversized-fragment
        if ${pkgs.bash}/bin/bash ${../../modules/lib/check-closure-limits.sh} \
          exact-members oversized-fragment services.graft oversized \
          2> fragment-limit-error; then
          echo "131073-byte closure fragment was accepted" >&2
          exit 1
        fi
        grep -F "is 131073 bytes; limit is 131072" fragment-limit-error

        test "$(wc -l < ${largeClosureInfo}/store-paths)" -eq 512
        test "$(grep -c '^Volume=' ${largeClosureSource})" -eq 513

        mkdir source-system source-user generated-system generated-user
        cp ${closureNixosSource} source-system/closure-system.container
        cp ${largeClosureSource} source-system/closure-large.container
        cp ${closureHomeManagerSource} source-user/closure-user.container

        QUADLET_UNIT_DIRS="$PWD/source-system" \
          ${pkgs.podman}/libexec/podman/quadlet \
          generated-system generated-system generated-system
        QUADLET_UNIT_DIRS="$PWD/source-user" \
          ${pkgs.podman}/libexec/podman/quadlet -user \
          generated-user generated-user generated-user

        grep -Fq -- ${lib.escapeShellArg "-v ${closureRegularFile}:${closureRegularFile}:ro,bind,nodev,nosuid"} \
          generated-system/closure-system.service
        grep -Fq -- ${lib.escapeShellArg "-v ${closureRegularFile}:${closureRegularFile}:ro,bind,nodev,nosuid"} \
          generated-user/closure-user.service
        test "$(wc -c < generated-system/closure-large.service)" -lt 524288
        large_exec_size=$(grep '^ExecStart=' generated-system/closure-large.service | wc -c)
        test "$large_exec_size" -lt 196608

        mkdir -p runtime/systemd
        XDG_RUNTIME_DIR="$PWD/runtime" \
          SYSTEMD_UNIT_PATH="$PWD/generated-system:$PWD/generated-user:${pkgs.podman}/share/systemd/user:${pkgs.systemd}/example/systemd/user:${pkgs.systemd}/example/systemd/system" \
          ${lib.getExe' pkgs.systemd "systemd-analyze"} --user verify \
          generated-system/*.service generated-user/*.service

        cp generated-system/*.service generated-user/*.service "$out/"
      '';

  documentation-drift =
    pkgs.runCommand "graft-documentation-drift"
      {
        nativeBuildInputs = [ pkgs.python3 ];
      }
      ''
        python3 - \
          "${../../crates/graft/schema/graft-v1.schema.json}" \
          "${../../docs/capabilities.md}" \
          "${../../README.md}" \
          "${../../website/index.html}" \
          "${../../examples/quickstart/nixos/containers/graft-example.toml}" <<'PY'
        import html
        import json
        import re
        import sys
        import tomllib
        from collections import Counter
        from pathlib import Path

        schema_path, documentation_path, readme_path, website_path, fixture_path = map(
            Path, sys.argv[1:]
        )
        schema = json.loads(schema_path.read_text())
        definitions = schema["$defs"]

        def resolve_variants(node):
            if "$ref" in node:
                return resolve_variants(definitions[node["$ref"].rsplit("/", 1)[1]])
            variants = [
                variant
                for variant in node.get("anyOf", [])
                if variant.get("type") != "null"
            ]
            if variants:
                return [
                    resolved
                    for variant in variants
                    for resolved in resolve_variants(variant)
                ]
            return [node]

        def collect_fields(node, prefix, fields):
            for name, raw_property in node["properties"].items():
                path = f"{prefix}.{name}" if prefix else name
                resolved_properties = resolve_variants(raw_property)
                nested_properties = [
                    resolved
                    for resolved in resolved_properties
                    if resolved.get("properties") is not None
                ]
                if nested_properties:
                    for resolved in nested_properties:
                        collect_fields(resolved, path, fields)
                    continue

                fields.add(path)
                for resolved_property in resolved_properties:
                    items = resolved_property.get("items")
                    if items is None:
                        continue
                    for resolved_items in resolve_variants(items):
                        if resolved_items.get("properties") is not None:
                            collect_fields(resolved_items, f"{path}[]", fields)

        schema_fields = set()
        collect_fields(schema, "", schema_fields)

        documentation = documentation_path.read_text()
        start_marker = "<!-- supported-schema-fields:start -->"
        end_marker = "<!-- supported-schema-fields:end -->"
        if documentation.count(start_marker) != 1 or documentation.count(end_marker) != 1:
            raise SystemExit("capability documentation must contain exactly one supported-field marker pair")

        table = documentation.split(start_marker, 1)[1].split(end_marker, 1)[0]
        documented_fields = [
            line.split("`", 2)[1]
            for line in table.splitlines()
            if line.startswith("| `")
        ]
        duplicates = sorted(
            field for field, count in Counter(documented_fields).items() if count > 1
        )
        if duplicates:
            raise SystemExit(
                "duplicate supported capability field(s): " + ", ".join(duplicates)
            )

        documented_field_set = set(documented_fields)
        missing = sorted(schema_fields - documented_field_set)
        extra = sorted(documented_field_set - schema_fields)
        if missing or extra:
            messages = []
            if missing:
                messages.append("missing from capability documentation: " + ", ".join(missing))
            if extra:
                messages.append("absent from supported schema: " + ", ".join(extra))
            raise SystemExit("\n".join(messages))

        readme = readme_path.read_text()
        readme_examples = re.findall(r"```toml\n(.*?)```", readme, re.DOTALL)
        if len(readme_examples) != 1:
            raise SystemExit("README must contain exactly one TOML example")

        readme_example = tomllib.loads(readme_examples[0])
        fixture = tomllib.loads(fixture_path.read_text())
        expected_example = {
            "version": fixture["version"],
            "name": fixture["name"],
            "deploy": {"target": fixture["deploy"]["target"]},
            "config": {"runtime": fixture["config"]["runtime"]},
        }
        if readme_example != expected_example:
            raise SystemExit(
                "README example must exactly match the public quickstart projection"
            )

        website = website_path.read_text()
        website_examples = re.findall(r"<pre><code>(.*?)</code></pre>", website, re.DOTALL)
        if len(website_examples) != 1:
            raise SystemExit("website must contain exactly one complete TOML example")

        website_toml = html.unescape(re.sub(r"<[^>]+>", "", website_examples[0]))
        website_example = tomllib.loads(website_toml)
        if website_example != expected_example:
            raise SystemExit(
                "website example must exactly match the public quickstart projection"
            )

        hero_contract = (
            f'<p><b>version</b> = <span>{fixture["version"]}</span></p>',
            f'<p><b>name</b> = <span>{json.dumps(fixture["name"])}</span></p>',
            '<p class="terminal-section">[deploy]</p>',
            f'<p><b>target</b> = <span>{json.dumps(fixture["deploy"]["target"])}</span></p>',
            f'<p><b>packages</b> = <span>{json.dumps(fixture["config"]["runtime"]["packages"], separators=(",", ":"))}</span></p>',
        )
        missing_hero_contract = [
            fragment for fragment in hero_contract if fragment not in website
        ]
        if missing_hero_contract:
            raise SystemExit(
                "website hero differs from validated quickstart intent: "
                + ", ".join(missing_hero_contract)
            )
        PY
        touch $out
      '';

  network-runtime-rootfs = pkgs.runCommand "graft-network-runtime-rootfs" { } ''
    mkdir -p $out/bin $out/etc $out/tmp $out/www
    cp ${pkgs.pkgsStatic.busybox}/bin/busybox $out/bin/busybox
    for applet in httpd ip timeout true wget; do
      ln -s busybox "$out/bin/$applet"
    done
    echo shared-network-ok > $out/www/index.html
  '';

  quadlet-base =
    let
      sources = {
        system = {
          nix-check-plain-system = pkgs.writeText "nix-check-plain-system.container" nixosPlainRendered;
          escape-system = pkgs.writeText "escape-system.container" nixosEscapeRendered;
        };
        user = {
          nix-check-plain-user = pkgs.writeText "nix-check-plain-user.container" homeManagerPlainRendered;
          escape-user = pkgs.writeText "escape-user.container" homeManagerEscapeRendered;
        };
      };
      helper = quadletTestHelper { inherit sources; };
    in
    pkgs.runCommand "graft-quadlet-base" { } ''
      mkdir malformed-source malformed-output
      ${helper.setup}

      ${helper.generate "system" false}
      ${helper.generate "user" false}

      for scope in system user; do
        plain="generated-$scope/nix-check-plain-$scope.service"
        escaped="generated-$scope/escape-$scope.service"
        test -f "$plain"
        test -f "$escaped"
        test "$(grep -c '^ExecStart=' "$plain")" -eq 1
        test "$(grep -c '^ExecStart=' "$escaped")" -eq 1
        plain_exec=$(grep '^ExecStart=' "$plain")
        escaped_exec=$(grep '^ExecStart=' "$escaped")

        printf '%s\n' "$plain_exec" | grep -Fq -- "--name nix-check-plain-$scope"
        printf '%s\n' "$plain_exec" | grep -Fq -- "--security-opt=no-new-privileges"
        printf '%s\n' "$plain_exec" | grep -Fq -- "--cap-drop all"
        printf '%s\n' "$plain_exec" | grep -Fq -- "--read-only"
        printf '%s\n' "$plain_exec" | grep -Eq -- '--rootfs /nix/store/[^ ]+-env:O /bin/graft-pause$'

        printf '%s\n' "$escaped_exec" | grep -Fq -- "--name escape-$scope"
        grep -Fxq 'Environment="DOLLAR=cost $$5"' "$escaped"
        grep -Fxq 'Environment="PERCENT=100%%"' "$escaped"
        grep -Fxq "Restart=on-failure" "$escaped"
        grep -Fxq "RestartSec=15s" "$escaped"
        printf '%s\n' "$escaped_exec" | grep -Fq -- '--workdir /work%%space/$$HOME'
        printf '%s\n' "$escaped_exec" | grep -Fq -- '--user 100%%0:100%%0'
        printf '%s\n' "$escaped_exec" | grep -Fq -- '-v /tmp/graft-$$USER-%%n:/data$$HOME-%%h:ro,bind'
        printf '%s\n' "$escaped_exec" | grep -Fq -- '--env-file "/etc/graft/my\x20config.env"'
        printf '%s\n' "$escaped_exec" | grep -Fq -- '"cost\x20$$5" "foo\\.bar" "C:\\Temp" "say\x20\"hi\""'
      done
      grep '^ExecStart=' generated-system/escape-system.service \
        | grep -Fq -- '--publish 127.0.0.1:18%%080:80'
      grep '^ExecStart=' generated-user/escape-user.service \
        | grep -Fq -- '--publish 127.0.0.1:28%%080:80'

      ${helper.verify}

      cat > malformed-source/malformed.container <<'EOF'
      [Container]
      Rootfs=/tmp/rootfs
      UnsupportedSentinel=true
      EOF
      if QUADLET_UNIT_DIRS="$PWD/malformed-source" \
        ${pkgs.podman}/libexec/podman/quadlet \
        malformed-output malformed-output malformed-output \
        2> malformed-generator-error; then
        echo "malformed Quadlet source was accepted" >&2
        exit 1
      fi
      grep -Fq "unsupported key 'UnsupportedSentinel'" malformed-generator-error
      grep -Fq "processing encountered some errors" malformed-generator-error
      test ! -e malformed-output/malformed.service

      cp generated-system/*.service generated-user/*.service $out/
    '';

  quadlet-lifecycle =
    let
      sources = {
        system = {
          nix-check-system = pkgs.writeText "nix-check-system.container" nixosRendered;
          nix-check-timer-job-system = pkgs.writeText "nix-check-timer-job-system.container" nixosTimerJobRendered;
          nix-check-setup-system = pkgs.writeText "nix-check-setup-system.container" nixosSetupRendered;
        };
        user = {
          nix-check-user = pkgs.writeText "nix-check-user.container" homeManagerRendered;
          nix-check-timer-job-user = pkgs.writeText "nix-check-timer-job-user.container" homeManagerTimerJobRendered;
          nix-check-setup-user = pkgs.writeText "nix-check-setup-user.container" homeManagerSetupRendered;
        };
      };
      helper = quadletTestHelper { inherit sources; };
    in
    pkgs.runCommand "graft-quadlet-lifecycle" { } ''
      ${helper.setup}
      ${helper.generate "system" false}
      ${helper.generate "user" false}

      for scope in system user; do
        generated="generated-$scope"

        grep -Fx "Type=notify" "$generated/nix-check-$scope.service"
        grep -F -- "--sdnotify=conmon -d" "$generated/nix-check-$scope.service"

        grep -Fx "Type=oneshot" "$generated/nix-check-timer-job-$scope.service"
        grep -Fx "RemainAfterExit=no" "$generated/nix-check-timer-job-$scope.service"
        ! grep -F -- "--sdnotify=" "$generated/nix-check-timer-job-$scope.service"
        ! grep -E '^ExecStart=.* -d( |$)' "$generated/nix-check-timer-job-$scope.service"

        grep -Fx "Type=oneshot" "$generated/nix-check-setup-$scope.service"
        grep -Fx "RemainAfterExit=yes" "$generated/nix-check-setup-$scope.service"
        ! grep -F -- "--sdnotify=" "$generated/nix-check-setup-$scope.service"
        ! grep -E '^ExecStart=.* -d( |$)' "$generated/nix-check-setup-$scope.service"
      done

      ${helper.verify}
      cp generated-system/*.service generated-user/*.service $out/
    '';

  quadlet-activation =
    let
      sources = {
        system = {
          nix-check-system = pkgs.writeText "nix-check-system.container" nixosRendered;
          nix-check-startup-job-system = pkgs.writeText "nix-check-startup-job-system.container" nixosStartupJobRendered;
          nix-check-setup-system = pkgs.writeText "nix-check-setup-system.container" nixosSetupRendered;
          nix-check-timer-job-system = pkgs.writeText "nix-check-timer-job-system.container" nixosTimerJobRendered;
          nix-check-plain-system = pkgs.writeText "nix-check-plain-system.container" nixosPlainRendered;
          nix-check-network-owner-system = pkgs.writeText "nix-check-network-owner-system.container" nixosNetworkOwnerRendered;
          nix-check-network-client-system = pkgs.writeText "nix-check-network-client-system.container" nixosNetworkClientRendered;
        };
        user = {
          nix-check-user = pkgs.writeText "nix-check-user.container" homeManagerRendered;
          nix-check-startup-job-user = pkgs.writeText "nix-check-startup-job-user.container" homeManagerStartupJobRendered;
          nix-check-setup-user = pkgs.writeText "nix-check-setup-user.container" homeManagerSetupRendered;
          nix-check-timer-job-user = pkgs.writeText "nix-check-timer-job-user.container" homeManagerTimerJobRendered;
          nix-check-plain-user = pkgs.writeText "nix-check-plain-user.container" homeManagerPlainRendered;
          nix-check-network-owner-user = pkgs.writeText "nix-check-network-owner-user.container" homeManagerNetworkOwnerRendered;
          nix-check-network-client-user = pkgs.writeText "nix-check-network-client-user.container" homeManagerNetworkClientRendered;
        };
      };
      helper = quadletTestHelper { inherit sources; };
    in
    pkgs.runCommand "graft-quadlet-activation" { } ''
      mkdir persistent foreign
      ${helper.setup}

      for source in source-system/nix-check-plain-system.container source-user/nix-check-plain-user.container; do
        rootfs="$(sed -n 's|^Rootfs=\(.*\):O$|\1|p' "$source")"
        test -n "$rootfs"
        test -d "$rootfs/tmp"
        test -d "$rootfs/var/tmp"
      done

      ${helper.generate "system" false}
      ${helper.generate "user" false}

      grep -F -- '--tmpfs /run/graft-system:rw,noexec,nosuid,nodev,mode=0750,size=64M --tmpfs /tmp/graft-system:rw,noexec,nosuid,nodev' \
        generated-system/nix-check-system.service
      grep -F -- '--tmpfs /run/graft-user:rw,noexec,nosuid,nodev,mode=0750,size=64M --tmpfs /tmp/graft-user:rw,noexec,nosuid,nodev' \
        generated-user/nix-check-user.service
      grep -F -- '-v /tmp/graft-system-data:/data:rw,bind -v /tmp/graft-system-config:/config:ro,bind -v /system-cache' \
        generated-system/nix-check-system.service
      grep -F -- '-v /tmp/graft-user-data:/data:rw,bind -v /tmp/graft-user-config:/config:ro,bind -v /user-cache' \
        generated-user/nix-check-user.service
      for scope in system user; do
        service="generated-$scope/nix-check-plain-$scope.service"
        grep -E -- "^ExecStart=.* --security-opt=no-new-privileges( |$)" "$service"
        grep -E -- "^ExecStart=.* --cap-drop all( |$)" "$service"
        grep -E -- "^ExecStart=.* --read-only( |$)" "$service"
      done

      for unit in nix-check nix-check-startup-job nix-check-setup nix-check-network-client; do
        test -L "generated-system/multi-user.target.wants/$unit-system.service"
        test "$(readlink "generated-system/multi-user.target.wants/$unit-system.service")" = \
          "../$unit-system.service"
        test -L "generated-user/default.target.wants/$unit-user.service"
        test "$(readlink "generated-user/default.target.wants/$unit-user.service")" = \
          "../$unit-user.service"
      done

      for unit in nix-check-timer-job nix-check-plain nix-check-network-owner; do
        test ! -e "generated-system/multi-user.target.wants/$unit-system.service"
        test ! -e "generated-user/default.target.wants/$unit-user.service"
      done

      grep -Fx "Requires=nix-check-network-owner-system.service" \
        generated-system/nix-check-network-client-system.service
      grep -Fx "Requires=nix-check-network-owner-user.service" \
        generated-user/nix-check-network-client-user.service

      ${helper.verify}

      touch persistent/workload-state foreign/foreign.service
      rm source-system/nix-check-startup-job-system.container source-user/nix-check-startup-job-user.container
      sed '/^\[Install\]$/,$d' \
        ${sources.system.nix-check-startup-job-system} \
        > source-system/nix-check-startup-job-system.container
      sed '/^\[Install\]$/,$d' \
        ${sources.user.nix-check-startup-job-user} \
        > source-user/nix-check-startup-job-user.container
      rm -rf generated-system generated-user
      mkdir generated-system generated-user

      ${helper.generate "system" false}
      ${helper.generate "user" false}

      test ! -e generated-system/multi-user.target.wants/nix-check-startup-job-system.service
      test ! -e generated-user/default.target.wants/nix-check-startup-job-user.service
      test -e persistent/workload-state
      test -e foreign/foreign.service

      rm source-system/nix-check-startup-job-system.container source-user/nix-check-startup-job-user.container
      cp ${sources.system.nix-check-startup-job-system} source-system/nix-check-startup-job-system.container
      cp ${sources.user.nix-check-startup-job-user} source-user/nix-check-startup-job-user.container
      rm -rf generated-system generated-user
      mkdir generated-system generated-user

      ${helper.generate "system" false}
      ${helper.generate "user" false}

      test -L generated-system/multi-user.target.wants/nix-check-startup-job-system.service
      test -L generated-user/default.target.wants/nix-check-startup-job-user.service
      test -e persistent/workload-state
      test -e foreign/foreign.service

      cp -a generated-system generated-user persistent foreign $out/
    '';

  quadlet-network =
    let
      sources = {
        system = {
          nix-check-network-owner-system = pkgs.writeText "nix-check-network-owner-system.container" nixosNetworkOwnerRendered;
          nix-check-network-client-system = pkgs.writeText "nix-check-network-client-system.container" nixosNetworkClientRendered;
          nix-check-network-none-system = pkgs.writeText "nix-check-network-none-system.container" nixosNetworkNoneRendered;
        };
        user = {
          nix-check-network-owner-user = pkgs.writeText "nix-check-network-owner-user.container" homeManagerNetworkOwnerRendered;
          nix-check-network-client-user = pkgs.writeText "nix-check-network-client-user.container" homeManagerNetworkClientRendered;
          nix-check-network-none-user = pkgs.writeText "nix-check-network-none-user.container" homeManagerNetworkNoneRendered;
        };
      };
      helper = quadletTestHelper { inherit sources; };
    in
    pkgs.runCommand "graft-quadlet-network" { } ''
      ${helper.setup}
      ${helper.generate "system" false}
      ${helper.generate "user" false}

      for scope in system user; do
        generated="generated-$scope"
        owner="nix-check-network-owner-$scope.service"
        client="$generated/nix-check-network-client-$scope.service"

        grep -Fx "Requires=$owner" "$client"
        grep -Fx "After=$owner" "$client"
        grep -E "^ExecStart=.* --network container:nix-check-network-owner-$scope( |$)" "$client"
        grep -E "^ExecStart=.* --network none( |$)" "$generated/nix-check-network-none-$scope.service"
      done

      ${helper.verify}
      cp generated-system/*.service generated-user/*.service $out/
    '';

  quadlet-cdi =
    let
      sources = {
        system = {
          nix-check-cdi-system = pkgs.writeText "nix-check-cdi-system.container" nixosCdiRendered;
        };
        user = {
          nix-check-cdi-user = pkgs.writeText "nix-check-cdi-user.container" homeManagerCdiRendered;
        };
      };
      helper = quadletTestHelper { inherit sources; };
    in
    pkgs.runCommand "graft-quadlet-cdi" { } ''
      ${helper.setup}

      for source in source-system/nix-check-cdi-system.container source-user/nix-check-cdi-user.container; do
        test "$(grep -c '^AddDevice=' "$source")" = 2
        test "$(grep -n '^AddDevice=' "$source" | cut -d: -f2-)" = \
          $'AddDevice=nvidia.com/gpu=all\nAddDevice=vendor.example/device_class=device-1.2'
        test "$(grep -c '^DropCapability=' "$source")" = 1
        grep -Fx "DropCapability=all" "$source"
        test "$(grep -c '^AddCapability=' "$source")" = 2
        test "$(grep -n '^AddCapability=' "$source" | cut -d: -f2-)" = \
          $'AddCapability=CAP_NET_BIND_SERVICE\nAddCapability=CAP_CHOWN'
      done
      grep -Fx "ReadOnly=true" source-system/nix-check-cdi-system.container
      grep -Fx "NoNewPrivileges=true" source-system/nix-check-cdi-system.container
      grep -Fx "ReadOnly=false" source-user/nix-check-cdi-user.container
      grep -Fx "NoNewPrivileges=false" source-user/nix-check-cdi-user.container

      ${helper.generate "system" false}
      ${helper.generate "user" false}

      for scope in system user; do
        service="generated-$scope/nix-check-cdi-$scope.service"
        grep -F -- \
          "--device nvidia.com/gpu=all --device vendor.example/device_class=device-1.2" \
          "$service"
        grep -E -- "^ExecStart=.* --cap-drop all( |$)" "$service"
        grep -F -- \
          "--cap-add cap_net_bind_service --cap-add cap_chown" \
          "$service"
      done
      grep -E -- "^ExecStart=.* --security-opt=no-new-privileges( |$)" \
        generated-system/nix-check-cdi-system.service
      grep -E -- "^ExecStart=.* --read-only( |$)" generated-system/nix-check-cdi-system.service
      ! grep -F -- "--security-opt=no-new-privileges" generated-user/nix-check-cdi-user.service
      ! grep -E -- "^ExecStart=.* --read-only( |$)" generated-user/nix-check-cdi-user.service

      ${helper.verify}
      cp generated-system/*.service generated-user/*.service $out/
    '';

  quadlet-dependencies =
    let
      sources = {
        system = {
          dependency-owner-system = pkgs.writeText "dependency-owner-system.container" nixosDependencyOwnerRendered;
          dependency-client-system = pkgs.writeText "dependency-client-system.container" nixosDependencyClientRendered;
        };
        user = {
          dependency-owner-user = pkgs.writeText "dependency-owner-user.container" homeManagerDependencyOwnerRendered;
          dependency-client-user = pkgs.writeText "dependency-client-user.container" homeManagerDependencyClientRendered;
        };
      };
      helper = quadletTestHelper { inherit sources; };
    in
    pkgs.runCommand "graft-quadlet-dependencies" { } ''
      ${helper.setup}
      ${helper.generate "system" false}
      ${helper.generate "user" false}

      cat > generated-system/graft-foreign-system.service <<'EOF'
      [Service]
      Type=oneshot
      ExecStart=${lib.getExe' pkgs.coreutils "true"}
      RemainAfterExit=yes
      EOF
      cat > generated-user/graft-foreign-user.service <<'EOF'
      [Service]
      Type=oneshot
      ExecStart=${lib.getExe' pkgs.coreutils "true"}
      RemainAfterExit=yes
      EOF
      cp generated-system/graft-foreign-system.service \
        generated-system/graft-bound-system.service
      cp generated-user/graft-foreign-user.service \
        generated-user/graft-bound-user.service

      for scope in system user; do
        generated="generated-$scope"
        owner="dependency-owner-$scope.service"
        foreign="graft-foreign-$scope.service"
        bound="graft-bound-$scope.service"
        client="$generated/dependency-client-$scope.service"

        test "$(grep -Fxc "Requires=$owner" "$client")" = 1
        test "$(grep -Fxc "After=$owner" "$client")" = 1
        grep -Fx "PartOf=$owner" "$client"
        grep -Fx "Wants=$foreign" "$client"
        grep -Fx "Before=$foreign" "$client"
        grep -Fx "BindsTo=$bound" "$client"
        ! grep -F "dependency-client-$scope.service" "$generated/$owner"
      done

      test -L generated-system/multi-user.target.wants/dependency-client-system.service
      test -L generated-user/default.target.wants/dependency-client-user.service

      ${helper.verify}
      cp generated-system/*.service generated-user/*.service $out/
    '';
}
