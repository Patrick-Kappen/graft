{
  description = "Graft — NixOS Podman Quadlet containers from TOML";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";

  outputs = { self, nixpkgs, ... }:
    let
      systems = [ "x86_64-linux" "aarch64-linux" ];
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

      packages = forAllSystems (system:
        let pkgs = nixpkgs.legacyPackages.${system};
        in {
          default = pkgs.rustPlatform.buildRustPackage {
            pname = "graft";
            version = "0.1.0-alpha.1";
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

      checks = forAllSystems (system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          lib = pkgs.lib;

          moduleTestOptions = { lib, ... }: {
            options.assertions = lib.mkOption {
              type = lib.types.listOf lib.types.anything;
              default = [ ];
            };

            options.environment.etc = lib.mkOption {
              type = lib.types.attrsOf (lib.types.submodule {
                options.text = lib.mkOption { type = lib.types.str; };
              });
              default = { };
            };

            options.xdg.configFile = lib.mkOption {
              type = lib.types.attrsOf (lib.types.submodule {
                options.text = lib.mkOption { type = lib.types.str; };
              });
              default = { };
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
                };
              }
            ];
          };

          nixosRendered = nixosEval.config.environment.etc."containers/systemd/system.container".text;
          nixosPlainRendered = nixosEval.config.environment.etc."containers/systemd/plain-system.container".text;
          nixosEscapeRendered = nixosEval.config.environment.etc."containers/systemd/escape-system.container".text;
          homeManagerRendered = homeManagerEval.config.xdg.configFile."containers/systemd/user.container".text;
          homeManagerPlainRendered = homeManagerEval.config.xdg.configFile."containers/systemd/plain-user.container".text;
          homeManagerEscapeRendered = homeManagerEval.config.xdg.configFile."containers/systemd/escape-user.container".text;
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
        in
        {
          nixos-module-eval = assert lib.hasInfix "ContainerName=nix-check-system" nixosRendered;
            assert lib.hasInfix "HostName=nix-check-system.local" nixosRendered;
            assert lib.hasInfix "User=1000" nixosRendered;
            assert lib.hasInfix "Group=1000" nixosRendered;
            assert lib.hasInfix "WorkingDir=/workspace" nixosRendered;
            assert lib.hasInfix expectedEnvironmentLines nixosRendered;
            assert lib.hasInfix "EnvironmentFile=/etc/graft/system.env\nEnvironmentFile=/run/graft/shared.env" nixosRendered;
            assert lib.hasInfix "Volume=/system-cache\nVolume=/tmp/graft-system-data:/data\nVolume=/tmp/graft-system-config:/config:ro" nixosRendered;
            assert lib.hasInfix "PublishPort=127.0.0.1:18080:80\nPublishPort=18443:443/tcp" nixosRendered;
            assert lib.hasInfix "\n[Service]\nRestartSec=10s\nTimeoutStartSec=2m\nTimeoutStopSec=30s" nixosRendered;
            assert lib.hasInfix "ContainerName=escape-system" nixosEscapeRendered;
            assert lib.hasInfix "HostName=escape%%system.local" nixosEscapeRendered;
            assert lib.hasInfix "User=100%%0" nixosEscapeRendered;
            assert lib.hasInfix "Group=100%%0" nixosEscapeRendered;
            assert lib.hasInfix "WorkingDir=/work%%space/$$HOME" nixosEscapeRendered;
            assert lib.hasInfix "Exec=/bin/echo 'pre$\${HOME}post' 100%% 'cost $$5'" nixosEscapeRendered;
            assert lib.hasInfix expectedEscapedEnvironmentLines nixosEscapeRendered;
            assert lib.hasInfix "EnvironmentFile=/etc/graft/$$USER-%%n.env" nixosEscapeRendered;
            assert lib.hasInfix "Volume=/tmp/graft-$$USER-%%n:/data$$HOME-%%h:ro%%z" nixosEscapeRendered;
            assert lib.hasInfix "PublishPort=127.0.0.1:18%%080:80" nixosEscapeRendered;
            assert lib.hasInfix "\n[Service]\nRestartSec=10%%s" nixosEscapeRendered;
            assert !lib.hasInfix "HostName=" nixosPlainRendered;
            assert !lib.hasInfix "User=" nixosPlainRendered;
            assert !lib.hasInfix "Group=" nixosPlainRendered;
            assert !lib.hasInfix "WorkingDir=" nixosPlainRendered;
            assert !lib.hasInfix "Environment=" nixosPlainRendered;
            assert !lib.hasInfix "EnvironmentFile=" nixosPlainRendered;
            assert !lib.hasInfix "Volume=/system-cache" nixosPlainRendered;
            assert !lib.hasInfix "Volume=/tmp/graft-system-data:/data" nixosPlainRendered;
            assert !lib.hasInfix "Volume=/tmp/graft-system-config:/config:ro" nixosPlainRendered;
            assert !lib.hasInfix "PublishPort=" nixosPlainRendered;
            assert !lib.hasInfix "RestartSec=" nixosPlainRendered;
            assert !lib.hasInfix "TimeoutStartSec=" nixosPlainRendered;
            assert !lib.hasInfix "TimeoutStopSec=" nixosPlainRendered;
            assert !(nixosEval.config.environment.etc ? "containers/systemd/user.container");
            assert !(nixosEval.config.environment.etc ? "containers/systemd/escape-user.container");
            pkgs.writeText "graft-nixos-module-eval" nixosRendered;

          home-manager-module-eval = assert lib.hasInfix "ContainerName=nix-check-user" homeManagerRendered;
            assert lib.hasInfix "HostName=nix-check-user.local" homeManagerRendered;
            assert lib.hasInfix "User=1000" homeManagerRendered;
            assert lib.hasInfix "Group=1000" homeManagerRendered;
            assert lib.hasInfix "WorkingDir=/workspace" homeManagerRendered;
            assert lib.hasInfix expectedEnvironmentLines homeManagerRendered;
            assert lib.hasInfix "EnvironmentFile=/etc/graft/user.env\nEnvironmentFile=/run/graft/shared.env" homeManagerRendered;
            assert lib.hasInfix "Volume=/user-cache\nVolume=/tmp/graft-user-data:/data\nVolume=/tmp/graft-user-config:/config:ro" homeManagerRendered;
            assert lib.hasInfix "PublishPort=127.0.0.1:28080:80\nPublishPort=28443:443/tcp" homeManagerRendered;
            assert lib.hasInfix "\n[Service]\nRestartSec=10s\nTimeoutStartSec=2m\nTimeoutStopSec=30s" homeManagerRendered;
            assert lib.hasInfix "ContainerName=escape-user" homeManagerEscapeRendered;
            assert lib.hasInfix "HostName=escape%%user.local" homeManagerEscapeRendered;
            assert lib.hasInfix "User=100%%0" homeManagerEscapeRendered;
            assert lib.hasInfix "Group=100%%0" homeManagerEscapeRendered;
            assert lib.hasInfix "WorkingDir=/work%%space/$$HOME" homeManagerEscapeRendered;
            assert lib.hasInfix "Exec=/bin/echo 'pre$\${HOME}post' 100%% 'cost $$5'" homeManagerEscapeRendered;
            assert lib.hasInfix expectedEscapedEnvironmentLines homeManagerEscapeRendered;
            assert lib.hasInfix "EnvironmentFile=/etc/graft/$$USER-%%n.env" homeManagerEscapeRendered;
            assert lib.hasInfix "Volume=/tmp/graft-$$USER-%%n:/data$$HOME-%%h:ro%%z" homeManagerEscapeRendered;
            assert lib.hasInfix "PublishPort=127.0.0.1:28%%080:80" homeManagerEscapeRendered;
            assert lib.hasInfix "\n[Service]\nRestartSec=10%%s" homeManagerEscapeRendered;
            assert !lib.hasInfix "HostName=" homeManagerPlainRendered;
            assert !lib.hasInfix "User=" homeManagerPlainRendered;
            assert !lib.hasInfix "Group=" homeManagerPlainRendered;
            assert !lib.hasInfix "WorkingDir=" homeManagerPlainRendered;
            assert !lib.hasInfix "Environment=" homeManagerPlainRendered;
            assert !lib.hasInfix "EnvironmentFile=" homeManagerPlainRendered;
            assert !lib.hasInfix "Volume=/user-cache" homeManagerPlainRendered;
            assert !lib.hasInfix "Volume=/tmp/graft-user-data:/data" homeManagerPlainRendered;
            assert !lib.hasInfix "Volume=/tmp/graft-user-config:/config:ro" homeManagerPlainRendered;
            assert !lib.hasInfix "PublishPort=" homeManagerPlainRendered;
            assert !lib.hasInfix "RestartSec=" homeManagerPlainRendered;
            assert !lib.hasInfix "TimeoutStartSec=" homeManagerPlainRendered;
            assert !lib.hasInfix "TimeoutStopSec=" homeManagerPlainRendered;
            assert !(homeManagerEval.config.xdg.configFile ? "containers/systemd/system.container");
            assert !(homeManagerEval.config.xdg.configFile ? "containers/systemd/escape-system.container");
            pkgs.writeText "graft-home-manager-module-eval" homeManagerRendered;
        }
      );

      devShells = forAllSystems (system:
        let pkgs = nixpkgs.legacyPackages.${system};
        in rec {
          ci = pkgs.mkShell {
            packages = with pkgs; [
              actionlint
              cargo
              cargo-audit
              clippy
              mdbook
              rustc
              rustfmt
            ];
          };

          default = ci;
        }
      );
    };
}
