{ pkgs, graftPackage }:

let
  inherit (pkgs) lib;
  configRoot = ../nix/runtime-filesystem;

  moduleTestOptions = { lib, ... }: {
    options = {
      assertions = lib.mkOption {
        type = lib.types.listOf lib.types.anything;
        default = [ ];
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

  userEval = lib.evalModules {
    specialArgs = { inherit pkgs; };
    modules = [
      moduleTestOptions
      ../../modules/home-manager.nix
      {
        programs.graft = {
          enable = true;
          package = graftPackage;
          inherit configRoot;
        };
      }
    ];
  };

  userSource = userEval.config.xdg.configFile."containers/systemd/filesystem-user.container".text;
in
{
  name = "graft-filesystem-runtime";

  nodes.machine = {
    imports = [ ../../modules/nixos.nix ];

    services.graft = {
      enable = true;
      package = graftPackage;
      inherit configRoot;
    };

    virtualisation = {
      diskSize = 3072;
      memorySize = 2048;
      podman.enable = true;
    };

    users.mutableUsers = false;
    users.users.graftfs = {
      isNormalUser = true;
      uid = 1000;
      linger = true;
    };

    environment.etc."containers/systemd/users/1000/filesystem-user.container".text = userSource;

    systemd.tmpfiles.rules = [
      "d /srv/graft-filesystem 0755 root root -"
      "d /srv/graft-filesystem/readonly 0755 root root -"
      "d /srv/graft-filesystem/readonly/submount 0755 root root -"
      "f /srv/graft-filesystem/readonly/base 0644 root root - base"
      "d /srv/graft-filesystem/writable 0755 root root -"
      "d /srv/graft-filesystem/control 0755 root root -"
      "d /home/graftfs/readonly 0755 graftfs users -"
      "d /home/graftfs/readonly/submount 0755 graftfs users -"
      "f /home/graftfs/readonly/base 0644 graftfs users - base"
      "d /home/graftfs/writable 0755 graftfs users -"
      "d /home/graftfs/control 0755 graftfs users -"
    ];
  };

  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")
    machine.wait_for_unit("user@1000.service")
    machine.wait_for_file("/run/user/1000/bus")

    machine.succeed("mount -t tmpfs -o mode=0755,size=1M tmpfs /srv/graft-filesystem/readonly/submount")
    machine.succeed("touch /srv/graft-filesystem/readonly/submount/host-marker")

    user_systemctl = "setpriv --reuid=1000 --regid=$(id -g 1000) --clear-groups env XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus systemctl --user"

    with subtest("source units expose fixed typed filesystem intent"):
        system_source = "/etc/containers/systemd/filesystem-system.container"
        user_source = "/etc/containers/systemd/users/1000/filesystem-user.container"
        for source in [system_source, user_source]:
            machine.succeed(f"grep -F ':ro,bind' {source}")
            machine.succeed(f"grep -F ':rw,bind' {source}")
            machine.succeed(f"grep -F 'Tmpfs=/scratch:rw,noexec,nosuid,nodev,mode=1777,size=16M' {source}")
            machine.succeed(f"grep -F 'Volume=graft-filesystem-runtime:/named:rw' {source}")
            machine.succeed(f"grep -Fx 'Volume=/anonymous' {source}")

    with subtest("rootful filesystems enforce access and lifecycle"):
        machine.succeed("systemctl start filesystem-system.service")
        machine.succeed("test -e /srv/graft-filesystem/writable/container-marker")
        machine.succeed("touch /srv/graft-filesystem/control/second")
        machine.succeed("systemctl start filesystem-system.service")
        machine.succeed("test $(systemctl show filesystem-system.service -P Result) = success")

    with subtest("rootless filesystems enforce access and lifecycle"):
        machine.succeed(f"{user_systemctl} start filesystem-user.service")
        machine.succeed("test -e /home/graftfs/writable/container-marker")
        machine.succeed("touch /home/graftfs/control/second")
        machine.succeed(f"{user_systemctl} start filesystem-user.service")
        machine.succeed(f"test $({user_systemctl} show filesystem-user.service -P Result) = success")
  '';
}
