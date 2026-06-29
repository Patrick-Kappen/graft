{
  description = "podman-agent-container";

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
        podman-agent-container = pkgs.buildGoModule {
          pname = "podman-agent-container";
          version = "0.1.0";
          src = ./.;

          vendorHash = "sha256-QCFEllD/+ak4LBRimQ5QcVeoZfOiZmgvee8YWEPc+qY=";
          subPackages = [ "cmd/podman-agent-container" ];

          nativeBuildInputs = [ pkgs.makeWrapper ];

          postInstall = ''
            mkdir -p $out/share/podman-agent-container
            cp config.example.toml $out/share/podman-agent-container/config.example.toml

            wrapProgram $out/bin/podman-agent-container \
              --prefix PATH : ${
                pkgs.lib.makeBinPath [
                  pkgs.nix
                  pkgs.podman
                  pkgs.systemd
                ]
              }

            ln -s $out/bin/podman-agent-container $out/bin/pac
          '';
        };

        default = podman-agent-container;
      });

      apps = forAllSystems (pkgs: {
        pac = {
          type = "app";
          program = "${self.packages.${pkgs.system}.podman-agent-container}/bin/pac";
        };

        podman-agent-container = {
          type = "app";
          program = "${self.packages.${pkgs.system}.podman-agent-container}/bin/podman-agent-container";
        };

        default = self.apps.${pkgs.system}.pac;
      });

      checks = forAllSystems (
        pkgs:
        let
          inherit (pkgs) lib;
          system = pkgs.stdenv.hostPlatform.system;

          mkNixos =
            configRoot:
            nixpkgs.lib.nixosSystem {
              inherit system;
              modules = [
                self.nixosModules.default
                (_: {
                  system.stateVersion = "26.05";
                  services.podman-agent-container = {
                    enable = true;
                    inherit configRoot;
                  };
                })
              ];
            };

          containerEtcNames =
            configRoot:
            builtins.filter (name: builtins.match "containers/systemd/.*" name != null) (
              builtins.attrNames (mkNixos configRoot).config.environment.etc
            );

          mkHome =
            configRoot:
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
                  config.programs.podman-agent-container = {
                    enable = true;
                    inherit configRoot;
                  };
                })
              ];
              specialArgs = { inherit pkgs; };
            };

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
            discoveryNixos.config.environment.etc."containers/systemd/pac-test-child.container".source;
          parentSetNixos = mkNixos ./tests/nixos-module/parents-set;
          renderedParentSetQuadlet =
            parentSetNixos.config.environment.etc."containers/systemd/pac-test-parent-set.container".source;
          parentRemoveNixos = mkNixos ./tests/nixos-module/parents-remove;
          renderedParentRemoveQuadlet =
            parentRemoveNixos.config.environment.etc."containers/systemd/pac-test-parent-remove.container".source;
          parentCycleEval = builtins.tryEval (
            builtins.deepSeq (containerEtcNames ./tests/nixos-module/parent-cycle) true
          );
          childrenAddNixos = mkNixos ./tests/nixos-module/children-add;
          renderedChildrenAddQuadlet =
            childrenAddNixos.config.environment.etc."containers/systemd/pac-test-children-add.container".source;
          childrenSetNixos = mkNixos ./tests/nixos-module/children-set;
          renderedChildrenSetQuadlet =
            childrenSetNixos.config.environment.etc."containers/systemd/pac-test-children-set.container".source;
          childrenRemoveNixos = mkNixos ./tests/nixos-module/children-remove;
          renderedChildrenRemoveQuadlet =
            childrenRemoveNixos.config.environment.etc."containers/systemd/pac-test-children-remove.container".source;
          packageOpsNixos = mkNixos ./tests/nixos-module/package-ops;
          renderedPackageOpsQuadlet =
            packageOpsNixos.config.environment.etc."containers/systemd/pac-test-package-ops.container".source;
          configRootExampleNixos = mkNixos ./examples/config-root;
          renderedConfigRootExampleQuadlet =
            configRootExampleNixos.config.environment.etc."containers/systemd/pac-demo.container".source;
          homeExample = mkHome ./tests/home-manager/config-root;
          renderedHomeExampleQuadlet =
            homeExample.config.xdg.configFile."containers/systemd/pac-user-demo.container".source;
          unknownPackageNixos = mkNixos ./tests/nixos-module/unknown-package;
          unknownPackageAssertionMessages = map (assertion: assertion.message) (
            builtins.filter (assertion: !assertion.assertion) unknownPackageNixos.config.assertions
          );

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
                "containers/systemd/pac-test-active.container"
                "containers/systemd/pac-test-child.container"
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
                "ContainerName=pac-demo"
                "Environment=PAC_DEMO=1"
                "ReadOnly=true"
                "Network=none"
                "/bin/hostname"
              ];

          home-manager-module-config-root-discovery =
            checkList "home-manager-module-config-root-discovery"
              (homeConfigNames ./tests/home-manager/config-root)
              [ "containers/systemd/pac-user-demo.container" ];

          home-manager-module-render =
            checkFileContains "home-manager-module-render" renderedHomeExampleQuadlet
              [
                "ContainerName=pac-user-demo"
                "Environment=PAC_USER_DEMO=1"
                "echo from user quadlet"
              ];

          nixos-module-duplicate-name-assertion =
            if
              builtins.elem "services.podman-agent-container: active TOML config names must be unique." duplicateAssertionMessages
            then
              pkgs.runCommand "nixos-module-duplicate-name-assertion" { } "touch $out"
            else
              throw "expected duplicate active TOML names to produce a NixOS assertion";

          nixos-module-unknown-package-assertion =
            if lib.any (message: lib.hasInfix "unknown package" message) unknownPackageAssertionMessages then
              pkgs.runCommand "nixos-module-unknown-package-assertion" { } "touch $out"
            else
              throw "expected unknown TOML runtime package to produce a NixOS assertion";
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

            echo "podman-agent-container dev shell"
            echo "Go: $(go version)"
          '';
        };
      });
    };
}
