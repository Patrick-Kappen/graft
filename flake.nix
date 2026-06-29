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
