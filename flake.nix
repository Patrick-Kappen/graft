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
          homeManagerRendered = homeManagerEval.config.xdg.configFile."containers/systemd/user.container".text;
          homeManagerPlainRendered = homeManagerEval.config.xdg.configFile."containers/systemd/plain-user.container".text;
        in
        {
          nixos-module-eval = assert lib.hasInfix "ContainerName=nix-check-system" nixosRendered;
            assert lib.hasInfix "HostName=nix-check-system.local" nixosRendered;
            assert lib.hasInfix "User=1000" nixosRendered;
            assert !lib.hasInfix "HostName=" nixosPlainRendered;
            assert !lib.hasInfix "User=" nixosPlainRendered;
            assert !(nixosEval.config.environment.etc ? "containers/systemd/user.container");
            pkgs.writeText "graft-nixos-module-eval" nixosRendered;

          home-manager-module-eval = assert lib.hasInfix "ContainerName=nix-check-user" homeManagerRendered;
            assert lib.hasInfix "HostName=nix-check-user.local" homeManagerRendered;
            assert lib.hasInfix "User=1000" homeManagerRendered;
            assert !lib.hasInfix "HostName=" homeManagerPlainRendered;
            assert !lib.hasInfix "User=" homeManagerPlainRendered;
            assert !(homeManagerEval.config.xdg.configFile ? "containers/systemd/system.container");
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
