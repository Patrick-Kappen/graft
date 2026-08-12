{ graftPackage, ... }:

{
  name = "graft-cdi-runtime";

  nodes.machine = {
    imports = [ ../../modules/nixos.nix ];

    services.graft = {
      enable = true;
      package = graftPackage;
      hostId = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20";
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

    with subtest("resolved Quadlet contains CDI and hardening intent"):
        source = "/etc/containers/systemd/graft-cdi-runtime.container"
        machine.succeed(f"grep -Fx 'AddDevice=graft.test/device=fake' {source}")
        machine.succeed(f"grep -Fx 'ReadOnly=true' {source}")
        machine.succeed(f"grep -Fx 'DropCapability=all' {source}")
        machine.succeed(f"grep -Fx 'NoNewPrivileges=true' {source}")

    with subtest("CDI and hardening reach the container"):
        machine.succeed("systemctl start graft-cdi-runtime.service")
        machine.succeed("test $(systemctl show graft-cdi-runtime.service -P Result) = success")
        machine.succeed("test $(systemctl show graft-cdi-runtime.service -P ActiveState) = inactive")
  '';
}
