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
      graftCargoMetadata = builtins.fromTOML (builtins.readFile ./crates/graft/Cargo.toml);
      graftVersion = graftCargoMetadata.package.version;
      protocolSourceLines = nixpkgs.lib.splitString "\n" (
        builtins.readFile ./crates/graft/src/protocol/types.rs
      );
      parseProtocolConstant =
        name:
        let
          matches = builtins.filter (match: match != null) (
            map (
              line: builtins.match "[[:space:]]*pub const ${name}: u16 = ([0-9]+);[[:space:]]*" line
            ) protocolSourceLines
          );
        in
        if builtins.length matches == 1 then
          builtins.fromJSON (builtins.elemAt (builtins.head matches) 0)
        else
          throw "Expected exactly one ${name} u16 constant in crates/graft/src/protocol/types.rs";
      rustWorkerApiRange = {
        major = parseProtocolConstant "PROTOCOL_MAJOR";
        min_minor = parseProtocolConstant "PROTOCOL_MIN_MINOR";
        max_minor = parseProtocolConstant "PROTOCOL_MAX_MINOR";
      };
      requireWorkerApiParity =
        rustRange: nixRange:
        if rustRange == nixRange then
          nixRange
        else
          throw "Graft Rust/Nix worker API range mismatch: ${builtins.toJSON rustRange} != ${builtins.toJSON nixRange}";
      graftWorkerApiRange = requireWorkerApiParity rustWorkerApiRange (import ./nix/worker-api-range.nix);
      # Flake revision metadata is deterministic and does not invoke VCS tools.
      # Archives and dirty trees intentionally use the stable provenance marker.
      buildIdForRevision = revision: if revision == null then "source" else revision;
      graftBuildId = buildIdForRevision (self.rev or null);
      requireVersionParity =
        cargoVersion: nixVersion:
        if cargoVersion == nixVersion then
          nixVersion
        else
          throw "Graft Cargo/Nix version mismatch: ${cargoVersion} != ${nixVersion}";
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
            version = requireVersionParity graftVersion graftVersion;
            src = ./crates/graft;
            cargoLock.lockFile = ./crates/graft/Cargo.lock;

            passthru.graftBuildId = graftBuildId;
            passthru.graftWorkerApiRange = graftWorkerApiRange;

            postInstall = ''
              test -x "$out/bin/graft-worker"
              test -x "$out/bin/graft-manifest-render"
              test -x "$out/bin/graft-manifest-publish-system"
              test -x "$out/bin/graft-manifest-publish-user"
            '';

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
          system-manifest-publication-runtime-test = pkgs.testers.runNixOSTest (
            import ./tests/nixos/system-manifest-publication.nix {
              inherit pkgs graftPackage;
            }
          );
          notify-protocol-runtime-test = pkgs.testers.runNixOSTest (
            import ./tests/nixos/notify-protocol.nix {
              inherit pkgs graftPackage;
            }
          );
          notify-protocol-debug-runtime-test = pkgs.testers.runNixOSTest (
            import ./tests/nixos/notify-protocol.nix {
              inherit pkgs graftPackage;
              debugLogging = true;
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
          closure-runtime-test = pkgs.testers.runNixOSTest (
            import ./tests/nixos/closure.nix {
              inherit pkgs graftPackage;
            }
          );
        }
      );

      checks = forAllSystems (
        system:
        import ./nix/checks {
          inherit
            self
            system
            graftVersion
            graftBuildId
            graftWorkerApiRange
            rustWorkerApiRange
            buildIdForRevision
            requireVersionParity
            requireWorkerApiParity
            ;
          pkgs = nixpkgs.legacyPackages.${system};
          graftPackage = self.packages.${system}.default;
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
