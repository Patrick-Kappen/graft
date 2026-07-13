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

    with subtest("resolved Quadlet contains CDI and hardening intent"):
        source = "/etc/containers/systemd/cdi-runtime-system.container"
        machine.succeed(f"grep -Fx 'AddDevice=graft.test/device=fake' {source}")
        machine.succeed(f"grep -Fx 'ReadOnly=true' {source}")
        machine.succeed(f"grep -Fx 'DropCapability=all' {source}")
        machine.succeed(f"grep -Fx 'NoNewPrivileges=true' {source}")

    with subtest("CDI and hardening reach the container"):
        machine.succeed("systemctl start cdi-runtime-system.service")
        machine.succeed("test $(systemctl show cdi-runtime-system.service -P Result) = success")
        machine.succeed("test $(systemctl show cdi-runtime-system.service -P ActiveState) = inactive")
  '';
}
