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
        in
        {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "graft";
            version = "0.2.0-alpha.1";
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

          nixosRendered = nixosEval.config.environment.etc."containers/systemd/system.container".text;
          nixosPlainRendered =
            nixosEval.config.environment.etc."containers/systemd/plain-system.container".text;
          nixosEscapeRendered =
            nixosEval.config.environment.etc."containers/systemd/escape-system.container".text;
          nixosHostRendered =
            nixosEval.config.environment.etc."containers/systemd/host-system.container".text;
          homeManagerRendered =
            homeManagerEval.config.xdg.configFile."containers/systemd/user.container".text;
          homeManagerPlainRendered =
            homeManagerEval.config.xdg.configFile."containers/systemd/plain-user.container".text;
          homeManagerEscapeRendered =
            homeManagerEval.config.xdg.configFile."containers/systemd/escape-user.container".text;
          homeManagerHostRendered =
            homeManagerEval.config.xdg.configFile."containers/systemd/host-user.container".text;
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
            "\n[Service]\nRestartSec=10s\nTimeoutStartSec=2m\nTimeoutStopSec=30s"
          ];
          commonEscapedInfixes = [
            "User=100%%0"
            "Group=100%%0"
            "WorkingDir=/work%%space/$$HOME"
            "Exec=\"/bin/echo\" \"pre$\${HOME}post\" \"100%%\" \"cost $$5\" \"foo\\\\.bar\" \"C:\\\\Temp\" \"say \\\"hi\\\"\""
            expectedEscapedEnvironmentLines
            "EnvironmentFile=\"/etc/graft/$$USER-%%n.env\"\nEnvironmentFile=\"/etc/graft/my config.env\"\nEnvironmentFile=\"/etc/graft/env\\\\prod.env\""
            "Volume=/tmp/graft-$$USER-%%n:/data$$HOME-%%h:ro%%z"
            "\n[Service]\nRestartSec=15s"
          ];
          commonPlainMissingInfixes = [
            "HostName="
            "User="
            "Group="
            "WorkingDir="
            "Environment="
            "EnvironmentFile="
            "PublishPort="
            "RestartSec="
            "TimeoutStartSec="
            "TimeoutStopSec="
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
            assertHasInfixes rendered (commonRenderedInfixes ++ renderedInfixes)
            && assertHasInfixes escapeRendered (commonEscapedInfixes ++ escapeInfixes)
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
                "Volume=/system-cache\nVolume=/tmp/graft-system-data:/data\nVolume=/tmp/graft-system-config:/config:ro"
                "PublishPort=127.0.0.1:18080:80\nPublishPort=18443:443/tcp"
              ];
              escapeInfixes = [
                "ContainerName=escape-system"
                "HostName=escape%%system.local"
                "PublishPort=127.0.0.1:18%%080:80"
              ];
              plainMissingInfixes = [
                "Volume=/system-cache"
                "Volume=/tmp/graft-system-data:/data"
                "Volume=/tmp/graft-system-config:/config:ro"
              ];
            };
            assert assertHasInfixes nixosHostRendered [
              "ContainerName=nix-check-host-system"
              "HostName=host-system.local"
            ];
            assert !(nixosEval.config.environment.etc ? "containers/systemd/user.container");
            assert !(nixosEval.config.environment.etc ? "containers/systemd/escape-user.container");
            assert !(nixosEval.config.environment.etc ? "containers/systemd/host-user.container");
            assert duplicateFilenameNixosFails;
            assert duplicateNameNixosFails;
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
                "Volume=/user-cache\nVolume=/tmp/graft-user-data:/data\nVolume=/tmp/graft-user-config:/config:ro"
                "PublishPort=127.0.0.1:28080:80\nPublishPort=28443:443/tcp"
              ];
              escapeInfixes = [
                "ContainerName=escape-user"
                "HostName=escape%%user.local"
                "PublishPort=127.0.0.1:28%%080:80"
              ];
              plainMissingInfixes = [
                "Volume=/user-cache"
                "Volume=/tmp/graft-user-data:/data"
                "Volume=/tmp/graft-user-config:/config:ro"
              ];
            };
            assert assertHasInfixes homeManagerHostRendered [
              "ContainerName=nix-check-host-user"
              "HostName=host-user.local"
            ];
            assert !(homeManagerEval.config.xdg.configFile ? "containers/systemd/system.container");
            assert !(homeManagerEval.config.xdg.configFile ? "containers/systemd/escape-system.container");
            assert !(homeManagerEval.config.xdg.configFile ? "containers/systemd/host-system.container");
            assert duplicateFilenameHomeManagerFails;
            assert duplicateNameHomeManagerFails;
            pkgs.writeText "graft-home-manager-module-eval" homeManagerRendered;
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
              cargo-nextest
              clippy
              deadnix
              git
              gitleaks
              llvmPackages.llvm
              markdownlint-cli2
              mdbook
              nixfmt
              rustc
              rustfmt
              statix
              taplo
              zizmor
            ];
          };

          default = ci;
        }
      );
    };
}
