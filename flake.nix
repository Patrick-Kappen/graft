{
  description = "Graft — NixOS Podman Quadlet containers from TOML";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { self, nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = nixpkgs.lib.genAttrs systems;
    in
    {
      nixosModules.graft = { lib, pkgs, ... }: {
        imports = [ ./modules/nixos.nix ];
        services.graft.package = lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      };
      homeManagerModules.graft = { lib, pkgs, ... }: {
        imports = [ ./modules/home-manager.nix ];
        programs.graft.package = lib.mkDefault self.packages.${pkgs.stdenv.hostPlatform.system}.default;
      };

      packages = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          graftPackage = pkgs.rustPlatform.buildRustPackage {
            pname = "graft";
            version = "0.3.0-alpha.1";
            src = ./crates/graft;
            cargoLock.lockFile = ./crates/graft/Cargo.lock;

            meta = {
              description = "TOML-driven Podman Quadlet containers, built from the Nix store";
              homepage = "https://github.com/Patrick-Kappen/graft";
              license = pkgs.lib.licenses.asl20;
              mainProgram = "graft";
              platforms = pkgs.lib.platforms.linux;
            };
          };
        in
        {
          default = graftPackage;
        }
        // pkgs.lib.optionalAttrs (system == "x86_64-linux") {
          activation-runtime-test = pkgs.testers.runNixOSTest (
            import ./tests/nixos/activation.nix {
              inherit pkgs graftPackage;
            }
          );
          notify-protocol-runtime-test = pkgs.testers.runNixOSTest (
            import ./tests/nixos/notify-protocol.nix {
              inherit pkgs graftPackage;
            }
          );
          cdi-runtime-test = pkgs.testers.runNixOSTest (
            import ./tests/nixos/cdi.nix {
              inherit graftPackage;
            }
          );
          filesystem-runtime-test = pkgs.testers.runNixOSTest (
            import ./tests/nixos/filesystem.nix {
              inherit pkgs graftPackage;
            }
          );
        }
      );

      checks = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          inherit (pkgs) lib;

          moduleTestOptions = { lib, ... }: {
            options = {
              assertions = lib.mkOption {
                type = lib.types.listOf lib.types.anything;
                default = [ ];
              };

              virtualisation.podman.enable = lib.mkOption {
                type = lib.types.bool;
                default = false;
              };

              environment.etc = lib.mkOption {
                type = lib.types.attrsOf (
                  lib.types.submodule {
                    options.text = lib.mkOption { type = lib.types.str; };
                  }
                );
                default = { };
              };

              xdg.configFile = lib.mkOption {
                type = lib.types.attrsOf (
                  lib.types.submodule {
                    options.text = lib.mkOption { type = lib.types.str; };
                  }
                );
                default = { };
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
                  configRoot = ./tests/nix/containers;
                  configRoots = [ ./tests/nix/containers-extra ];
                };
              }
            ];
          };

          homeManagerEval = lib.evalModules {
            specialArgs = { inherit pkgs; };
            modules = [
              moduleTestOptions
              self.homeManagerModules.graft
              {
                programs.graft = {
                  enable = true;
                  configRoot = ./tests/nix/containers;
                  configRoots = [ ./tests/nix/containers-extra ];
                };
              }
            ];
          };

          networkNixosEval = lib.evalModules {
            specialArgs = { inherit pkgs; };
            modules = [
              moduleTestOptions
              self.nixosModules.graft
              {
                services.graft = {
                  enable = true;
                  configRoot = ./tests/nix/network;
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
                  configRoot = ./tests/nix/network;
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
                  configRoot = ./tests/nix/dependencies;
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
                  configRoot = ./tests/nix/dependencies;
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
                  configRoot = ./tests/nix/cdi;
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
                  configRoot = ./tests/nix/cdi;
                };
              }
            ];
          };

          quickstartNixosEval = lib.evalModules {
            specialArgs = { inherit pkgs; };
            modules = [
              moduleTestOptions
              self.nixosModules.graft
              ./examples/quickstart/nixos/module.nix
            ];
          };

          quickstartHomeManagerEval = lib.evalModules {
            specialArgs = { inherit pkgs; };
            modules = [
              moduleTestOptions
              self.homeManagerModules.graft
              ./examples/quickstart/home-manager/module.nix
            ];
          };

          nixosRendered = nixosEval.config.environment.etc."containers/systemd/system.container".text;
          nixosPlainRendered =
            nixosEval.config.environment.etc."containers/systemd/plain-system.container".text;
          nixosEscapeRendered =
            nixosEval.config.environment.etc."containers/systemd/escape-system.container".text;
          nixosHostRendered =
            nixosEval.config.environment.etc."containers/systemd/host-system.container".text;
          nixosTimerJobRendered =
            nixosEval.config.environment.etc."containers/systemd/timer-job-system.container".text;
          nixosStartupJobRendered =
            nixosEval.config.environment.etc."containers/systemd/startup-job-system.container".text;
          nixosSetupRendered =
            nixosEval.config.environment.etc."containers/systemd/setup-system.container".text;
          nixosNetworkOwnerRendered =
            networkNixosEval.config.environment.etc."containers/systemd/network-owner-system.container".text;
          nixosNetworkClientRendered =
            networkNixosEval.config.environment.etc."containers/systemd/network-client-system.container".text;
          nixosNetworkNoneRendered =
            networkNixosEval.config.environment.etc."containers/systemd/network-none-system.container".text;
          homeManagerRendered =
            homeManagerEval.config.xdg.configFile."containers/systemd/user.container".text;
          homeManagerPlainRendered =
            homeManagerEval.config.xdg.configFile."containers/systemd/plain-user.container".text;
          homeManagerEscapeRendered =
            homeManagerEval.config.xdg.configFile."containers/systemd/escape-user.container".text;
          homeManagerHostRendered =
            homeManagerEval.config.xdg.configFile."containers/systemd/host-user.container".text;
          homeManagerTimerJobRendered =
            homeManagerEval.config.xdg.configFile."containers/systemd/timer-job-user.container".text;
          homeManagerStartupJobRendered =
            homeManagerEval.config.xdg.configFile."containers/systemd/startup-job-user.container".text;
          homeManagerSetupRendered =
            homeManagerEval.config.xdg.configFile."containers/systemd/setup-user.container".text;
          homeManagerNetworkOwnerRendered =
            networkHomeManagerEval.config.xdg.configFile."containers/systemd/network-owner-user.container".text;
          homeManagerNetworkClientRendered =
            networkHomeManagerEval.config.xdg.configFile."containers/systemd/network-client-user.container".text;
          homeManagerNetworkNoneRendered =
            networkHomeManagerEval.config.xdg.configFile."containers/systemd/network-none-user.container".text;
          nixosDependencyOwnerRendered =
            dependencyNixosEval.config.environment.etc."containers/systemd/dependency-owner-system.container".text;
          nixosDependencyClientRendered =
            dependencyNixosEval.config.environment.etc."containers/systemd/dependency-client-system.container".text;
          homeManagerDependencyOwnerRendered =
            dependencyHomeManagerEval.config.xdg.configFile."containers/systemd/dependency-owner-user.container".text;
          homeManagerDependencyClientRendered =
            dependencyHomeManagerEval.config.xdg.configFile."containers/systemd/dependency-client-user.container".text;
          nixosCdiRendered =
            cdiNixosEval.config.environment.etc."containers/systemd/cdi-system.container".text;
          homeManagerCdiRendered =
            cdiHomeManagerEval.config.xdg.configFile."containers/systemd/cdi-user.container".text;
          quickstartNixosRendered =
            quickstartNixosEval.config.environment.etc."containers/systemd/graft-example.container".text;
          quickstartHomeManagerRendered =
            quickstartHomeManagerEval.config.xdg.configFile."containers/systemd/graft-example.container".text;
          expectedQuickstartInfixes = [
            "ContainerName=graft-example"
            ''Exec="bash" "-c" "echo graft-example-ready; exec /bin/graft-pause"''
            "Volume=/nix/store:/nix/store:ro"
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
                    configRoot = ./tests/nix/containers;
                    configRoots = extraRoots;
                  };
                }
              ];
            };
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
                    configRoot = ./tests/nix/containers;
                    configRoots = extraRoots;
                  };
                }
              ];
            };
          duplicateFilenameNixosEval = evalNixosWithRoots [ ./tests/nix/duplicate-filename ];
          duplicateNameNixosEval = evalNixosWithRoots [ ./tests/nix/duplicate-name ];
          duplicateFilenameHomeManagerEval = evalHomeManagerWithRoots [ ./tests/nix/duplicate-filename ];
          duplicateNameHomeManagerEval = evalHomeManagerWithRoots [ ./tests/nix/duplicate-name ];
          duplicateFilenameNixosFails =
            !(builtins.tryEval (builtins.deepSeq duplicateFilenameNixosEval.config.environment.etc true))
            .success;
          duplicateNameNixosFails =
            !(builtins.tryEval (builtins.deepSeq duplicateNameNixosEval.config.environment.etc true)).success;
          duplicateFilenameHomeManagerFails =
            !(builtins.tryEval (builtins.deepSeq duplicateFilenameHomeManagerEval.config.xdg.configFile true))
            .success;
          duplicateNameHomeManagerFails =
            !(builtins.tryEval (builtins.deepSeq duplicateNameHomeManagerEval.config.xdg.configFile true))
            .success;
        in
        {
          nixos-module-eval =
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
              "Network=network-owner-system.container"
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
            assert !(nixosEval.config.environment.etc ? "containers/systemd/user.container");
            assert !(nixosEval.config.environment.etc ? "containers/systemd/escape-user.container");
            assert !(nixosEval.config.environment.etc ? "containers/systemd/host-user.container");
            assert !(nixosEval.config.environment.etc ? "containers/systemd/disabled-startup-system.container");
            assert !(nixosEval.config.environment.etc ? "containers/systemd/disabled-startup-user.container");
            assert duplicateFilenameNixosFails;
            assert duplicateNameNixosFails;
            assert quickstartNixosEval.config.virtualisation.podman.enable;
            assert assertHasInfixes quickstartNixosRendered (
              expectedQuickstartInfixes ++ [ ''Environment="GRAFT_EXAMPLE=nixos-system"'' ]
            );
            assert !(lib.hasInfix "WorkingDir=" quickstartNixosRendered);
            pkgs.writeText "graft-nixos-module-eval" nixosRendered;

          home-manager-module-eval =
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
              "Network=network-owner-user.container"
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
            assert !(homeManagerEval.config.xdg.configFile ? "containers/systemd/system.container");
            assert !(homeManagerEval.config.xdg.configFile ? "containers/systemd/escape-system.container");
            assert !(homeManagerEval.config.xdg.configFile ? "containers/systemd/host-system.container");
            assert
              !(homeManagerEval.config.xdg.configFile ? "containers/systemd/disabled-startup-system.container");
            assert
              !(homeManagerEval.config.xdg.configFile ? "containers/systemd/disabled-startup-user.container");
            assert duplicateFilenameHomeManagerFails;
            assert duplicateNameHomeManagerFails;
            assert assertHasInfixes quickstartHomeManagerRendered (
              expectedQuickstartInfixes ++ [ ''Environment="GRAFT_EXAMPLE=home-manager-user"'' ]
            );
            assert !(lib.hasInfix "WorkingDir=" quickstartHomeManagerRendered);
            pkgs.writeText "graft-home-manager-module-eval" homeManagerRendered;

          documentation-drift =
            pkgs.runCommand "graft-documentation-drift"
              {
                nativeBuildInputs = [ pkgs.python3 ];
              }
              ''
                python3 - \
                  "${./crates/graft/schema/graft-v1.schema.json}" \
                  "${./docs/capabilities.md}" \
                  "${./README.md}" \
                  "${./website/index.html}" \
                  "${./examples/quickstart/nixos/containers/graft-example.toml}" <<'PY'
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
                compared_paths = (
                    ("version",),
                    ("name",),
                    ("deploy", "target"),
                    ("config", "runtime", "packages"),
                    ("config", "runtime", "command"),
                )
                for path in compared_paths:
                    readme_value = readme_example
                    fixture_value = fixture
                    for component in path:
                        readme_value = readme_value[component]
                        fixture_value = fixture_value[component]
                    if readme_value != fixture_value:
                        raise SystemExit(
                            "README example differs from the validated quickstart at "
                            + ".".join(path)
                        )

                website = website_path.read_text()
                website_examples = re.findall(r"<pre><code>(.*?)</code></pre>", website, re.DOTALL)
                if len(website_examples) != 1:
                    raise SystemExit("website must contain exactly one complete TOML example")

                website_toml = html.unescape(re.sub(r"<[^>]+>", "", website_examples[0]))
                website_example = tomllib.loads(website_toml)
                for path in compared_paths:
                    website_value = website_example
                    fixture_value = fixture
                    for component in path:
                        website_value = website_value[component]
                        fixture_value = fixture_value[component]
                    if website_value != fixture_value:
                        raise SystemExit(
                            "website example differs from the validated quickstart at "
                            + ".".join(path)
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

          quadlet-lifecycle =
            let
              sources = {
                long-running-system = pkgs.writeText "long-running-system.container" nixosRendered;
                long-running-user = pkgs.writeText "long-running-user.container" homeManagerRendered;
                timer-job-system = pkgs.writeText "timer-job-system.container" nixosTimerJobRendered;
                timer-job-user = pkgs.writeText "timer-job-user.container" homeManagerTimerJobRendered;
                setup-system = pkgs.writeText "setup-system.container" nixosSetupRendered;
                setup-user = pkgs.writeText "setup-user.container" homeManagerSetupRendered;
              };
            in
            pkgs.runCommand "graft-quadlet-lifecycle" { } ''
              mkdir source-system source-user generated-system generated-user $out
              cp ${sources.long-running-system} source-system/long-running-system.container
              cp ${sources.timer-job-system} source-system/timer-job-system.container
              cp ${sources.setup-system} source-system/setup-system.container
              cp ${sources.long-running-user} source-user/long-running-user.container
              cp ${sources.timer-job-user} source-user/timer-job-user.container
              cp ${sources.setup-user} source-user/setup-user.container

              QUADLET_UNIT_DIRS="$PWD/source-system" \
                ${pkgs.podman}/libexec/podman/quadlet \
                generated-system generated-system generated-system
              QUADLET_UNIT_DIRS="$PWD/source-user" \
                ${pkgs.podman}/libexec/podman/quadlet -user \
                generated-user generated-user generated-user

              for scope in system user; do
                generated="generated-$scope"

                grep -Fx "Type=notify" "$generated/long-running-$scope.service"
                grep -F -- "--sdnotify=conmon -d" "$generated/long-running-$scope.service"

                grep -Fx "Type=oneshot" "$generated/timer-job-$scope.service"
                grep -Fx "RemainAfterExit=no" "$generated/timer-job-$scope.service"
                ! grep -F -- "--sdnotify=" "$generated/timer-job-$scope.service"
                ! grep -E '^ExecStart=.* -d( |$)' "$generated/timer-job-$scope.service"

                grep -Fx "Type=oneshot" "$generated/setup-$scope.service"
                grep -Fx "RemainAfterExit=yes" "$generated/setup-$scope.service"
                ! grep -F -- "--sdnotify=" "$generated/setup-$scope.service"
                ! grep -E '^ExecStart=.* -d( |$)' "$generated/setup-$scope.service"
              done

              mkdir -p runtime/systemd
              XDG_RUNTIME_DIR="$PWD/runtime" \
                SYSTEMD_UNIT_PATH="$PWD/generated-system:$PWD/generated-user:${pkgs.podman}/share/systemd/user:${pkgs.systemd}/example/systemd/user:${pkgs.systemd}/example/systemd/system" \
                ${lib.getExe' pkgs.systemd "systemd-analyze"} --user verify \
                generated-system/*.service generated-user/*.service
              cp generated-system/*.service generated-user/*.service $out/
            '';

          quadlet-activation =
            let
              sources = {
                long-running-system = pkgs.writeText "long-running-system.container" nixosRendered;
                startup-job-system = pkgs.writeText "startup-job-system.container" nixosStartupJobRendered;
                setup-system = pkgs.writeText "setup-system.container" nixosSetupRendered;
                timer-job-system = pkgs.writeText "timer-job-system.container" nixosTimerJobRendered;
                plain-system = pkgs.writeText "plain-system.container" nixosPlainRendered;
                network-owner-system = pkgs.writeText "network-owner-system.container" nixosNetworkOwnerRendered;
                network-client-system = pkgs.writeText "network-client-system.container" nixosNetworkClientRendered;
                long-running-user = pkgs.writeText "long-running-user.container" homeManagerRendered;
                startup-job-user = pkgs.writeText "startup-job-user.container" homeManagerStartupJobRendered;
                setup-user = pkgs.writeText "setup-user.container" homeManagerSetupRendered;
                timer-job-user = pkgs.writeText "timer-job-user.container" homeManagerTimerJobRendered;
                plain-user = pkgs.writeText "plain-user.container" homeManagerPlainRendered;
                network-owner-user = pkgs.writeText "network-owner-user.container" homeManagerNetworkOwnerRendered;
                network-client-user = pkgs.writeText "network-client-user.container" homeManagerNetworkClientRendered;
              };
            in
            pkgs.runCommand "graft-quadlet-activation" { } ''
              mkdir source-system source-user generated-system generated-user persistent foreign $out
              cp ${sources.long-running-system} source-system/long-running-system.container
              cp ${sources.startup-job-system} source-system/startup-job-system.container
              cp ${sources.setup-system} source-system/setup-system.container
              cp ${sources.timer-job-system} source-system/timer-job-system.container
              cp ${sources.plain-system} source-system/plain-system.container
              cp ${sources.network-owner-system} source-system/network-owner-system.container
              cp ${sources.network-client-system} source-system/network-client-system.container
              cp ${sources.long-running-user} source-user/long-running-user.container
              cp ${sources.startup-job-user} source-user/startup-job-user.container
              cp ${sources.setup-user} source-user/setup-user.container
              cp ${sources.timer-job-user} source-user/timer-job-user.container
              cp ${sources.plain-user} source-user/plain-user.container
              cp ${sources.network-owner-user} source-user/network-owner-user.container
              cp ${sources.network-client-user} source-user/network-client-user.container

              for source in source-system/plain-system.container source-user/plain-user.container; do
                rootfs="$(sed -n 's|^Rootfs=\(.*\):O$|\1|p' "$source")"
                test -n "$rootfs"
                test -d "$rootfs/tmp"
                test -d "$rootfs/var/tmp"
              done

              QUADLET_UNIT_DIRS="$PWD/source-system" \
                ${pkgs.podman}/libexec/podman/quadlet \
                generated-system generated-system generated-system
              QUADLET_UNIT_DIRS="$PWD/source-user" \
                ${pkgs.podman}/libexec/podman/quadlet -user \
                generated-user generated-user generated-user

              grep -F -- '--tmpfs /run/graft-system:rw,noexec,nosuid,nodev,mode=0750,size=64M --tmpfs /tmp/graft-system:rw,noexec,nosuid,nodev' \
                generated-system/long-running-system.service
              grep -F -- '--tmpfs /run/graft-user:rw,noexec,nosuid,nodev,mode=0750,size=64M --tmpfs /tmp/graft-user:rw,noexec,nosuid,nodev' \
                generated-user/long-running-user.service
              grep -F -- '-v /tmp/graft-system-data:/data:rw,bind -v /tmp/graft-system-config:/config:ro,bind -v /system-cache' \
                generated-system/long-running-system.service
              grep -F -- '-v /tmp/graft-user-data:/data:rw,bind -v /tmp/graft-user-config:/config:ro,bind -v /user-cache' \
                generated-user/long-running-user.service
              for scope in system user; do
                service="generated-$scope/plain-$scope.service"
                grep -E -- "^ExecStart=.* --security-opt=no-new-privileges( |$)" "$service"
                grep -E -- "^ExecStart=.* --cap-drop all( |$)" "$service"
                grep -E -- "^ExecStart=.* --read-only( |$)" "$service"
              done

              for unit in long-running startup-job setup network-client; do
                test -L "generated-system/multi-user.target.wants/$unit-system.service"
                test "$(readlink "generated-system/multi-user.target.wants/$unit-system.service")" = \
                  "../$unit-system.service"
                test -L "generated-user/default.target.wants/$unit-user.service"
                test "$(readlink "generated-user/default.target.wants/$unit-user.service")" = \
                  "../$unit-user.service"
              done

              for unit in timer-job plain network-owner; do
                test ! -e "generated-system/multi-user.target.wants/$unit-system.service"
                test ! -e "generated-user/default.target.wants/$unit-user.service"
              done

              grep -Fx "Requires=network-owner-system.service" \
                generated-system/network-client-system.service
              grep -Fx "Requires=network-owner-user.service" \
                generated-user/network-client-user.service

              mkdir -p runtime/systemd
              XDG_RUNTIME_DIR="$PWD/runtime" \
                SYSTEMD_UNIT_PATH="$PWD/generated-system:$PWD/generated-user:${pkgs.podman}/share/systemd/user:${pkgs.systemd}/example/systemd/user:${pkgs.systemd}/example/systemd/system" \
                ${lib.getExe' pkgs.systemd "systemd-analyze"} --user verify \
                generated-system/*.service generated-user/*.service

              touch persistent/workload-state foreign/foreign.service
              rm source-system/startup-job-system.container source-user/startup-job-user.container
              cp ${sources.plain-system} source-system/startup-job-system.container
              cp ${sources.plain-user} source-user/startup-job-user.container
              rm -rf generated-system generated-user
              mkdir generated-system generated-user

              QUADLET_UNIT_DIRS="$PWD/source-system" \
                ${pkgs.podman}/libexec/podman/quadlet \
                generated-system generated-system generated-system
              QUADLET_UNIT_DIRS="$PWD/source-user" \
                ${pkgs.podman}/libexec/podman/quadlet -user \
                generated-user generated-user generated-user

              test ! -e generated-system/multi-user.target.wants/startup-job-system.service
              test ! -e generated-user/default.target.wants/startup-job-user.service
              test -e persistent/workload-state
              test -e foreign/foreign.service

              rm source-system/startup-job-system.container source-user/startup-job-user.container
              cp ${sources.startup-job-system} source-system/startup-job-system.container
              cp ${sources.startup-job-user} source-user/startup-job-user.container
              rm -rf generated-system generated-user
              mkdir generated-system generated-user

              QUADLET_UNIT_DIRS="$PWD/source-system" \
                ${pkgs.podman}/libexec/podman/quadlet \
                generated-system generated-system generated-system
              QUADLET_UNIT_DIRS="$PWD/source-user" \
                ${pkgs.podman}/libexec/podman/quadlet -user \
                generated-user generated-user generated-user

              test -L generated-system/multi-user.target.wants/startup-job-system.service
              test -L generated-user/default.target.wants/startup-job-user.service
              test -e persistent/workload-state
              test -e foreign/foreign.service

              cp -a generated-system generated-user persistent foreign $out/
            '';

          quadlet-network =
            let
              sources = {
                owner-system = pkgs.writeText "network-owner-system.container" nixosNetworkOwnerRendered;
                client-system = pkgs.writeText "network-client-system.container" nixosNetworkClientRendered;
                none-system = pkgs.writeText "network-none-system.container" nixosNetworkNoneRendered;
                owner-user = pkgs.writeText "network-owner-user.container" homeManagerNetworkOwnerRendered;
                client-user = pkgs.writeText "network-client-user.container" homeManagerNetworkClientRendered;
                none-user = pkgs.writeText "network-none-user.container" homeManagerNetworkNoneRendered;
              };
            in
            pkgs.runCommand "graft-quadlet-network" { } ''
              mkdir source-system source-user generated-system generated-user $out
              cp ${sources.owner-system} source-system/network-owner-system.container
              cp ${sources.client-system} source-system/network-client-system.container
              cp ${sources.none-system} source-system/network-none-system.container
              cp ${sources.owner-user} source-user/network-owner-user.container
              cp ${sources.client-user} source-user/network-client-user.container
              cp ${sources.none-user} source-user/network-none-user.container

              QUADLET_UNIT_DIRS="$PWD/source-system" \
                ${pkgs.podman}/libexec/podman/quadlet \
                generated-system generated-system generated-system
              QUADLET_UNIT_DIRS="$PWD/source-user" \
                ${pkgs.podman}/libexec/podman/quadlet -user \
                generated-user generated-user generated-user

              for scope in system user; do
                generated="generated-$scope"
                owner="network-owner-$scope.service"
                client="$generated/network-client-$scope.service"

                grep -Fx "Requires=$owner" "$client"
                grep -Fx "After=$owner" "$client"
                grep -E "^ExecStart=.* --network container:nix-check-network-owner-$scope( |$)" "$client"
                grep -E "^ExecStart=.* --network none( |$)" "$generated/network-none-$scope.service"
              done

              mkdir -p runtime/systemd
              XDG_RUNTIME_DIR="$PWD/runtime" \
                SYSTEMD_UNIT_PATH="$PWD/generated-system:$PWD/generated-user:${pkgs.podman}/share/systemd/user:${pkgs.systemd}/example/systemd/user:${pkgs.systemd}/example/systemd/system" \
                ${lib.getExe' pkgs.systemd "systemd-analyze"} --user verify \
                generated-system/*.service generated-user/*.service
              cp generated-system/*.service generated-user/*.service $out/
            '';

          quadlet-cdi =
            let
              sources = {
                system = pkgs.writeText "cdi-system.container" nixosCdiRendered;
                user = pkgs.writeText "cdi-user.container" homeManagerCdiRendered;
              };
            in
            pkgs.runCommand "graft-quadlet-cdi" { } ''
              mkdir source-system source-user generated-system generated-user $out
              cp ${sources.system} source-system/cdi-system.container
              cp ${sources.user} source-user/cdi-user.container

              for source in source-system/cdi-system.container source-user/cdi-user.container; do
                test "$(grep -c '^AddDevice=' "$source")" = 2
                test "$(grep -n '^AddDevice=' "$source" | cut -d: -f2-)" = \
                  $'AddDevice=nvidia.com/gpu=all\nAddDevice=vendor.example/device_class=device-1.2'
                test "$(grep -c '^DropCapability=' "$source")" = 1
                grep -Fx "DropCapability=all" "$source"
                test "$(grep -c '^AddCapability=' "$source")" = 2
                test "$(grep -n '^AddCapability=' "$source" | cut -d: -f2-)" = \
                  $'AddCapability=CAP_NET_BIND_SERVICE\nAddCapability=CAP_CHOWN'
              done
              grep -Fx "ReadOnly=true" source-system/cdi-system.container
              grep -Fx "NoNewPrivileges=true" source-system/cdi-system.container
              grep -Fx "ReadOnly=false" source-user/cdi-user.container
              grep -Fx "NoNewPrivileges=false" source-user/cdi-user.container

              QUADLET_UNIT_DIRS="$PWD/source-system" \
                ${pkgs.podman}/libexec/podman/quadlet \
                generated-system generated-system generated-system
              QUADLET_UNIT_DIRS="$PWD/source-user" \
                ${pkgs.podman}/libexec/podman/quadlet -user \
                generated-user generated-user generated-user

              for scope in system user; do
                service="generated-$scope/cdi-$scope.service"
                grep -F -- \
                  "--device nvidia.com/gpu=all --device vendor.example/device_class=device-1.2" \
                  "$service"
                grep -E -- "^ExecStart=.* --cap-drop all( |$)" "$service"
                grep -F -- \
                  "--cap-add cap_net_bind_service --cap-add cap_chown" \
                  "$service"
              done
              grep -E -- "^ExecStart=.* --security-opt=no-new-privileges( |$)" \
                generated-system/cdi-system.service
              grep -E -- "^ExecStart=.* --read-only( |$)" generated-system/cdi-system.service
              ! grep -F -- "--security-opt=no-new-privileges" generated-user/cdi-user.service
              ! grep -E -- "^ExecStart=.* --read-only( |$)" generated-user/cdi-user.service

              mkdir -p runtime/systemd
              XDG_RUNTIME_DIR="$PWD/runtime" \
                SYSTEMD_UNIT_PATH="$PWD/generated-system:$PWD/generated-user:${pkgs.podman}/share/systemd/user:${pkgs.systemd}/example/systemd/user:${pkgs.systemd}/example/systemd/system" \
                ${lib.getExe' pkgs.systemd "systemd-analyze"} --user verify \
                generated-system/*.service generated-user/*.service
              cp generated-system/*.service generated-user/*.service $out/
            '';

          quadlet-dependencies =
            let
              sources = {
                owner-system = pkgs.writeText "dependency-owner-system.container" nixosDependencyOwnerRendered;
                client-system = pkgs.writeText "dependency-client-system.container" nixosDependencyClientRendered;
                owner-user = pkgs.writeText "dependency-owner-user.container" homeManagerDependencyOwnerRendered;
                client-user = pkgs.writeText "dependency-client-user.container" homeManagerDependencyClientRendered;
              };
            in
            pkgs.runCommand "graft-quadlet-dependencies" { } ''
              mkdir source-system source-user generated-system generated-user $out
              cp ${sources.owner-system} source-system/dependency-owner-system.container
              cp ${sources.client-system} source-system/dependency-client-system.container
              cp ${sources.owner-user} source-user/dependency-owner-user.container
              cp ${sources.client-user} source-user/dependency-client-user.container

              QUADLET_UNIT_DIRS="$PWD/source-system" \
                ${pkgs.podman}/libexec/podman/quadlet \
                generated-system generated-system generated-system
              QUADLET_UNIT_DIRS="$PWD/source-user" \
                ${pkgs.podman}/libexec/podman/quadlet -user \
                generated-user generated-user generated-user

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

              mkdir -p runtime/systemd
              XDG_RUNTIME_DIR="$PWD/runtime" \
                SYSTEMD_UNIT_PATH="$PWD/generated-system:$PWD/generated-user:${pkgs.podman}/share/systemd/user:${pkgs.systemd}/example/systemd/user:${pkgs.systemd}/example/systemd/system" \
                ${lib.getExe' pkgs.systemd "systemd-analyze"} --user verify \
                generated-system/*.service generated-user/*.service
              cp generated-system/*.service generated-user/*.service $out/
            '';
        }
      );

      devShells = forAllSystems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        rec {
          ci = pkgs.mkShell {
            packages = with pkgs; [
              actionlint
              cargo
              cargo-audit
              cargo-deny
              cargo-llvm-cov
              cargo-machete
              cargo-modules
              cargo-nextest
              clippy
              deadnix
              git
              gitleaks
              llvmPackages.llvm
              lychee
              markdownlint-cli2
              mdbook
              nixfmt
              podman
              rustc
              rustfmt
              shellcheck
              statix
              taplo
              typos
              zizmor
            ];
          };

          default = ci;
        }
      );
    };
}
