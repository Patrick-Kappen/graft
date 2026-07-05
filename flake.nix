{
  description = "graft";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs =
    { self, nixpkgs, ... }:
    let
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];
      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f (import nixpkgs { inherit system; }));
    in
    {
      packages = forAllSystems (pkgs: rec {
        graft = pkgs.buildGoModule {
          pname = "graft";
          version = "0.1.0";
          src = ./.;

          vendorHash = "sha256-pbA/AlBz3cQYRTMnQ/qBPcinYOKokrBLNhkbRTq54gE=";
          subPackages = [ "cmd/graft" ];

          # Bake the pinned nixpkgs store path into the binary so that
          # runtime package resolution always uses the same nixpkgs as the
          # build — no --impure, no host channel dependency.
          ldflags = [
            "-X github.com/zerodawn1990/graft/internal/cli.nixpkgsStorePath=${pkgs.path}"
          ];

          nativeBuildInputs = [ pkgs.makeWrapper ];

          postInstall = ''
            mkdir -p $out/share/graft
            cp config.example.toml $out/share/graft/config.example.toml

            wrapProgram $out/bin/graft \
              --prefix PATH : ${
                pkgs.lib.makeBinPath [
                  pkgs.diffutils
                  pkgs.nix
                  pkgs.podman
                  pkgs.systemd
                ]
              }
          '';
        };

        default = graft;
      });

      apps = forAllSystems (pkgs: {
        graft = {
          type = "app";
          program = "${self.packages.${pkgs.system}.graft}/bin/graft";
        };

        default = self.apps.${pkgs.system}.graft;
      });

      checks = forAllSystems (
        pkgs:
        let
          inherit (pkgs) lib;
          system = pkgs.stdenv.hostPlatform.system;

          mkNixosWith =
            graftCfg:
            nixpkgs.lib.nixosSystem {
              inherit system;
              modules = [
                self.nixosModules.default
                (_: {
                  system.stateVersion = "26.05";
                  services.graft = {
                    enable = true;
                  }
                  // graftCfg;
                })
              ];
            };
          mkNixos = configRoot: mkNixosWith { inherit configRoot; };

          containerEtcNames =
            configRoot:
            builtins.filter (name: builtins.match "containers/systemd/.*" name != null) (
              builtins.attrNames (mkNixos configRoot).config.environment.etc
            );

          mkHomeWith =
            graftCfg:
            lib.evalModules {
              modules = [
                self.homeManagerModules.default
                ({ lib, ... }: {
                  options = {
                    assertions = lib.mkOption {
                      type = lib.types.listOf lib.types.attrs;
                      default = [ ];
                    };
                    home.packages = lib.mkOption {
                      type = lib.types.listOf lib.types.package;
                      default = [ ];
                    };
                    xdg.configFile = lib.mkOption {
                      type = lib.types.attrsOf (
                        lib.types.submodule (_: {
                          options.source = lib.mkOption { type = lib.types.path; };
                        })
                      );
                      default = { };
                    };
                  };
                  config.programs.graft = {
                    enable = true;
                  }
                  // graftCfg;
                })
              ];
              specialArgs = { inherit pkgs; };
            };
          mkHome = configRoot: mkHomeWith { inherit configRoot; };

          homeConfigNames =
            configRoot:
            builtins.filter (name: builtins.match "containers/systemd/.*" name != null) (
              builtins.attrNames (mkHome configRoot).config.xdg.configFile
            );

          checkList =
            name: actual: expected:
            pkgs.runCommand name { } ''
              actual=${lib.escapeShellArg (builtins.toJSON actual)}
              expected=${lib.escapeShellArg (builtins.toJSON expected)}
              if [ "$actual" != "$expected" ]; then
                echo "expected: $expected" >&2
                echo "actual:   $actual" >&2
                exit 1
              fi
              touch $out
            '';

          duplicateNixos = mkNixos ./tests/nixos-module/duplicate;
          duplicateAssertionMessages = map (assertion: assertion.message) (
            builtins.filter (assertion: !assertion.assertion) duplicateNixos.config.assertions
          );
          discoveryNixos = mkNixos ./tests/nixos-module/discovery;
          renderedChildQuadlet =
            discoveryNixos.config.environment.etc."containers/systemd/graft-test-child.container".source;
          parentSetNixos = mkNixos ./tests/nixos-module/parents-set;
          renderedParentSetQuadlet =
            parentSetNixos.config.environment.etc."containers/systemd/graft-test-parent-set.container".source;
          parentRemoveNixos = mkNixos ./tests/nixos-module/parents-remove;
          renderedParentRemoveQuadlet =
            parentRemoveNixos.config.environment.etc."containers/systemd/graft-test-parent-remove.container".source;
          parentCycleEval = builtins.tryEval (
            builtins.deepSeq (containerEtcNames ./tests/nixos-module/parent-cycle) true
          );
          childrenAddNixos = mkNixos ./tests/nixos-module/children-add;
          renderedChildrenAddQuadlet =
            childrenAddNixos.config.environment.etc."containers/systemd/graft-test-children-add.container".source;
          childrenSetNixos = mkNixos ./tests/nixos-module/children-set;
          renderedChildrenSetQuadlet =
            childrenSetNixos.config.environment.etc."containers/systemd/graft-test-children-set.container".source;
          childrenRemoveNixos = mkNixos ./tests/nixos-module/children-remove;
          renderedChildrenRemoveQuadlet =
            childrenRemoveNixos.config.environment.etc."containers/systemd/graft-test-children-remove.container".source;
          packageOpsNixos = mkNixos ./tests/nixos-module/package-ops;
          renderedPackageOpsQuadlet =
            packageOpsNixos.config.environment.etc."containers/systemd/graft-test-package-ops.container".source;
          configRootExampleNixos = mkNixos ./examples/config-root;
          renderedConfigRootExampleQuadlet =
            configRootExampleNixos.config.environment.etc."containers/systemd/graft-demo.container".source;
          homeExample = mkHome ./tests/home-manager/config-root;
          renderedHomeExampleQuadlet =
            homeExample.config.xdg.configFile."containers/systemd/graft-user-demo.container".source;
          homeNetworkExample = mkHome ./tests/home-manager/network-unit;
          renderedHomeNetworkContainer =
            homeNetworkExample.config.xdg.configFile."containers/systemd/graft-network-demo.container".source;
          renderedHomeNetworkUnit =
            homeNetworkExample.config.xdg.configFile."containers/systemd/graft-internal.network".source;
          renderedHomeVolumeUnit =
            homeNetworkExample.config.xdg.configFile."containers/systemd/graft-cache.volume".source;
          unknownPackageNixos = mkNixos ./tests/nixos-module/unknown-package;
          unknownPackageAssertionMessages = map (assertion: assertion.message) (
            builtins.filter (assertion: !assertion.assertion) unknownPackageNixos.config.assertions
          );

          # Nix-native authoring: containers defined as Nix attrsets are serialized
          # to TOML and flow through the same resolver + renderer as file configs.
          nixContainersNixos = mkNixosWith {
            containers.graft-nix-demo = {
              config = {
                runtime = {
                  mode = "rootfs-store";
                  packages = [ "bashInteractive" ];
                  command = [
                    "bash"
                    "-lc"
                    "echo from nix"
                  ];
                };
                container.environment.FROM_NIX = "1";
              };
            };
          };
          nixContainerEtcNames = builtins.filter (name: builtins.match "containers/systemd/.*" name != null) (
            builtins.attrNames nixContainersNixos.config.environment.etc
          );
          renderedNixContainerQuadlet =
            nixContainersNixos.config.environment.etc."containers/systemd/graft-nix-demo.container".source;

          nixContainersHome = mkHomeWith {
            containers.graft-nix-user = {
              config = {
                runtime = {
                  mode = "rootfs-store";
                  packages = [ "bashInteractive" ];
                  command = [
                    "bash"
                    "-lc"
                    "echo from nix user"
                  ];
                };
                container.environment.FROM_NIX_USER = "1";
              };
            };
          };
          renderedNixContainerHomeQuadlet =
            nixContainersHome.config.xdg.configFile."containers/systemd/graft-nix-user.container".source;

          checkFileContains =
            name: file: needles:
            pkgs.runCommand name { } ''
              for needle in ${lib.escapeShellArgs needles}; do
                if ! grep -F -- "$needle" ${file} >/dev/null; then
                  echo "missing expected text in ${file}: $needle" >&2
                  echo "--- file ---" >&2
                  cat ${file} >&2
                  exit 1
                fi
              done
              touch $out
            '';

          checkFileNotContains =
            name: file: needles:
            pkgs.runCommand name { } ''
              for needle in ${lib.escapeShellArgs needles}; do
                if grep -F -- "$needle" ${file} >/dev/null; then
                  echo "unexpected text in ${file}: $needle" >&2
                  echo "--- file ---" >&2
                  cat ${file} >&2
                  exit 1
                fi
              done
              touch $out
            '';
        in
        {
          nixos-module-noop-config-root =
            checkList "nixos-module-noop-config-root" (containerEtcNames ./tests/nixos-module/noop-only)
              [ ];

          nixos-module-config-root-discovery =
            checkList "nixos-module-config-root-discovery" (containerEtcNames ./tests/nixos-module/discovery)
              [
                "containers/systemd/graft-test-active.container"
                "containers/systemd/graft-test-child.container"
              ];

          nixos-module-parent-merge-render =
            checkFileContains "nixos-module-parent-merge-render" renderedChildQuadlet
              [
                "Environment=FROM_PARENT=1"
                "Environment=FROM_CHILD=1"
                "echo from child"
              ];

          nixos-module-parents-set-contains =
            checkFileContains "nixos-module-parents-set-contains" renderedParentSetQuadlet
              [
                "Environment=FROM_B=1"
                "echo from set child"
              ];

          nixos-module-parents-set-excludes =
            checkFileNotContains "nixos-module-parents-set-excludes" renderedParentSetQuadlet
              [ "Environment=FROM_A=1" ];

          nixos-module-parents-remove-contains =
            checkFileContains "nixos-module-parents-remove-contains" renderedParentRemoveQuadlet
              [
                "Environment=FROM_B=1"
                "echo from remove child"
              ];

          nixos-module-parents-remove-excludes =
            checkFileNotContains "nixos-module-parents-remove-excludes" renderedParentRemoveQuadlet
              [ "Environment=FROM_A=1" ];

          nixos-module-parent-cycle-fails =
            if parentCycleEval.success then
              throw "expected parent cycle to fail NixOS evaluation"
            else
              pkgs.runCommand "nixos-module-parent-cycle-fails" { } "touch $out";

          nixos-module-children-add-render =
            checkFileContains "nixos-module-children-add-render" renderedChildrenAddQuadlet
              [
                "Environment=FROM_ENTRY=1"
                "Environment=FROM_CHILD_ADDON=1"
                "echo from child addon"
              ];

          nixos-module-children-set-contains =
            checkFileContains "nixos-module-children-set-contains" renderedChildrenSetQuadlet
              [ "Environment=FROM_ADDON_B=1" ];

          nixos-module-children-set-excludes =
            checkFileNotContains "nixos-module-children-set-excludes" renderedChildrenSetQuadlet
              [ "Environment=FROM_ADDON_A=1" ];

          nixos-module-children-remove-contains =
            checkFileContains "nixos-module-children-remove-contains" renderedChildrenRemoveQuadlet
              [ "Environment=FROM_ADDON_B=1" ];

          nixos-module-children-remove-excludes =
            checkFileNotContains "nixos-module-children-remove-excludes" renderedChildrenRemoveQuadlet
              [ "Environment=FROM_ADDON_A=1" ];

          nixos-module-package-ops-render =
            checkFileContains "nixos-module-package-ops-render" renderedPackageOpsQuadlet
              [
                "Exec=/nix/store/"
                "/bin/hostname"
              ];

          nixos-module-config-root-example =
            checkFileContains "nixos-module-config-root-example" renderedConfigRootExampleQuadlet
              [
                "ContainerName=graft-demo"
                "Environment=GRAFT_DEMO=1"
                "ReadOnly=true"
                "Network=none"
                "/bin/hostname"
              ];

          nixos-module-managed-rootfs-prepare =
            checkFileContains "nixos-module-managed-rootfs-prepare" renderedConfigRootExampleQuadlet
              [
                "Rootfs=%t/graft/graft-demo/rootfs"
                "ExecStartPre="
                "prepare-rootfs %t/graft/graft-demo/rootfs"
              ];

          home-manager-module-config-root-discovery =
            checkList "home-manager-module-config-root-discovery"
              (homeConfigNames ./tests/home-manager/config-root)
              [ "containers/systemd/graft-user-demo.container" ];

          home-manager-module-render =
            checkFileContains "home-manager-module-render" renderedHomeExampleQuadlet
              [
                "ContainerName=graft-user-demo"
                "Environment=GRAFT_USER_DEMO=1"
                "echo from user quadlet"
              ];

          home-manager-network-unit-container =
            checkFileContains "home-manager-network-unit-container" renderedHomeNetworkContainer
              [
                "Requires=graft-internal-network.service"
                "After=graft-internal-network.service"
                "Requires=graft-cache-volume.service"
                "After=graft-cache-volume.service"
                "Network=graft-internal.network"
                "Volume=graft-cache.volume:/cache:rw"
              ];

          home-manager-network-unit-render =
            checkFileContains "home-manager-network-unit-render" renderedHomeNetworkUnit
              [
                "NetworkName=graft-internal"
                "Driver=bridge"
                "Internal=true"
                "IPv6=false"
                "Subnet=10.89.0.0/24"
                "Gateway=10.89.0.1"
                "Options=mtu=1500"
                "Label=managed-by=graft"
              ];

          home-manager-volume-unit-render =
            checkFileContains "home-manager-volume-unit-render" renderedHomeVolumeUnit
              [
                "VolumeName=graft-cache"
                "Driver=local"
                "Copy=false"
                "Options=o=nodev"
                "Label=managed-by=graft"
              ];

          nixos-module-duplicate-name-assertion =
            if
              builtins.elem "services.graft: active TOML config names must be unique." duplicateAssertionMessages
            then
              pkgs.runCommand "nixos-module-duplicate-name-assertion" { } "touch $out"
            else
              throw "expected duplicate active TOML names to produce a NixOS assertion";

          nixos-module-unknown-package-assertion =
            if lib.any (message: lib.hasInfix "unknown package" message) unknownPackageAssertionMessages then
              pkgs.runCommand "nixos-module-unknown-package-assertion" { } "touch $out"
            else
              throw "expected unknown TOML runtime package to produce a NixOS assertion";

          nixos-module-nix-containers-active =
            checkList "nixos-module-nix-containers-active" nixContainerEtcNames
              [ "containers/systemd/graft-nix-demo.container" ];

          nixos-module-nix-containers-render =
            checkFileContains "nixos-module-nix-containers-render" renderedNixContainerQuadlet
              [
                "ContainerName=graft-nix-demo"
                "Environment=FROM_NIX=1"
                "echo from nix"
              ];

          home-manager-module-nix-containers-render =
            checkFileContains "home-manager-module-nix-containers-render" renderedNixContainerHomeQuadlet
              [
                "ContainerName=graft-nix-user"
                "Environment=FROM_NIX_USER=1"
                "echo from nix user"
              ];
        }
      );

      nixosModules.default = import ./nix/modules/nixos.nix { inherit self; };

      homeManagerModules.default = import ./nix/modules/home-manager.nix { inherit self; };

      devShells = forAllSystems (pkgs: {
        default = pkgs.mkShell {
          packages = with pkgs; [
            # Go development
            go
            gopls
            golangci-lint
            gotools
            delve

            # TOML / YAML / JSON tooling
            yq-go
            yamllint
            jq

            # Nix hygiene
            nixfmt
            nil
            statix
            deadnix

            # Podman / Quadlet smoke tests
            podman
            systemd
          ];

          shellHook = ''
            export GOPATH="$PWD/.go"
            export GOBIN="$GOPATH/bin"
            export PATH="$GOBIN:$PATH"

            echo "graft dev shell"
            echo "Go: $(go version)"
          '';
        };
      });
    };
}
