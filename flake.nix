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
