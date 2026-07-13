{ graftPackage, ... }:

{
  name = "graft-cdi-runtime";

  nodes.machine = {
    imports = [ ../../modules/nixos.nix ];

    services.graft = {
      enable = true;
      package = graftPackage;
      configRoot = ../nix/runtime-cdi;
    };

    virtualisation = {
      diskSize = 2048;
      memorySize = 1536;
      podman.enable = true;
    };

    environment.etc."cdi/graft-test.json".text = builtins.toJSON {
      cdiVersion = "1.0.0";
      kind = "graft.test/device";
      devices = [
        {
          name = "fake";
          containerEdits.env = [ "GRAFT_CDI_TEST=injected" ];
        }
      ];
    };
  };

  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")

    with subtest("resolved Quadlet contains the qualified CDI reference"):
        machine.succeed(
            "grep -Fx 'AddDevice=graft.test/device=fake' "
            "/etc/containers/systemd/cdi-runtime-system.container"
        )

    with subtest("fake CDI registry edits reach the container"):
        machine.succeed("systemctl start cdi-runtime-system.service")
        machine.succeed("test $(systemctl show cdi-runtime-system.service -P Result) = success")
        machine.succeed("test $(systemctl show cdi-runtime-system.service -P ActiveState) = inactive")
  '';
}
