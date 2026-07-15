{ pkgs, graftPackage }:

let
  inherit (pkgs) lib;
  unrelatedStorePath = pkgs.writeText "graft-closure-unrelated" "must-not-be-visible\n";
  configRoot = ../nix/runtime-closure;

  moduleTestOptions = { lib, ... }: {
    options = {
      assertions = lib.mkOption {
        type = lib.types.listOf lib.types.anything;
        default = [ ];
      };
      xdg.configFile = lib.mkOption {
        type = lib.types.attrsOf (
          lib.types.submodule {
            options.source = lib.mkOption { type = lib.types.path; };
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

  userSource = userEval.config.xdg.configFile."containers/systemd/closure-user.container".source;
in
{
  name = "graft-closure-scoped-store-runtime";

  nodes.machine = {
    imports = [ ../../modules/nixos.nix ];

    services.graft = {
      enable = true;
      package = graftPackage;
      inherit configRoot;
    };

    virtualisation = {
      diskSize = 4096;
      memorySize = 2048;
      podman.enable = true;
    };

    users.mutableUsers = false;
    users.users.graftclosure = {
      isNormalUser = true;
      uid = 1000;
      linger = true;
    };

    environment.etc."containers/systemd/users/1000/closure-user.container".source = userSource;
    system.extraDependencies = [ unrelatedStorePath ];

    systemd.tmpfiles.rules = [
      "d /var/lib/graft-closure 0755 root root -"
      "d /var/lib/graft-closure/system 0755 root root -"
      "d /home/graftclosure/result 0755 graftclosure users -"
    ];
  };

  testScript = ''
    machine.start()
    machine.wait_for_unit("multi-user.target")
    machine.wait_for_unit("user@1000.service")
    machine.wait_for_file("/run/user/1000/bus")
    machine.succeed("test -f ${unrelatedStorePath}")

    user_env = "setpriv --reuid=1000 --regid=$(id -g 1000) --clear-groups env HOME=/home/graftclosure USER=graftclosure XDG_RUNTIME_DIR=/run/user/1000 DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus"
    user_systemctl = f"{user_env} systemctl --user"
    user_podman = f"{user_env} podman"

    system_source = "/etc/containers/systemd/closure-system.container"
    user_source = "/etc/containers/systemd/users/1000/closure-user.container"

    with subtest("generated sources expose only sorted closure mounts"):
        for source in [system_source, user_source]:
            machine.succeed(f"test $(grep -Fc '/nix/store:/nix/store:ro,bind,nodev,nosuid' {source}) = 1")
            machine.fail(f"grep -Fx 'Volume=/nix/store:/nix/store:ro' {source}")
            machine.succeed(f"grep '^Volume=/nix/store/' {source} | tail -n +2 | cut -d: -f1 | sed 's/^Volume=//' | LC_ALL=C sort -c")
            machine.fail(f"grep -F '${unrelatedStorePath}:${unrelatedStorePath}' {source}")

    with subtest("rootful runtime sees exactly its declared closure"):
        machine.succeed("systemctl start closure-system.service")
        machine.wait_for_file("/var/lib/graft-closure/system/ready")
        machine.succeed("podman exec closure-system test -e /rootfs-write")
        machine.fail("podman exec closure-system test -e /nix/store/unexpected")
        machine.fail("podman exec closure-system test -e ${unrelatedStorePath}")
        machine.succeed("grep '^Volume=/nix/store/' /etc/containers/systemd/closure-system.container | tail -n +2 | cut -d: -f1 | sed 's#^Volume=/nix/store/##' | LC_ALL=C sort > /tmp/expected-system")
        machine.succeed("podman exec closure-system find /nix/store -mindepth 1 -maxdepth 1 -printf '%f\\n' | LC_ALL=C sort > /tmp/actual-system")
        machine.succeed("cmp /tmp/expected-system /tmp/actual-system")
        machine.succeed("podman exec closure-system cat /proc/mounts > /tmp/mounts-system")
        machine.succeed("awk -v expected=$(($(wc -l < /tmp/expected-system) + 1)) '$2 == \"/nix/store\" || index($2, \"/nix/store/\") == 1 { count++; if ((\",\" $4 \",\") !~ /,ro,/) exit 1 } END { if (count != expected) exit 1 }' /tmp/mounts-system")

    with subtest("rootless runtime sees exactly its declared closure"):
        machine.succeed(f"{user_systemctl} start closure-user.service")
        machine.wait_for_file("/home/graftclosure/result/ready")
        machine.succeed(f"{user_podman} exec closure-user test -e /rootfs-write")
        machine.fail(f"{user_podman} exec closure-user test -e /nix/store/unexpected")
        machine.fail(f"{user_podman} exec closure-user test -e ${unrelatedStorePath}")
        machine.succeed("grep '^Volume=/nix/store/' /etc/containers/systemd/users/1000/closure-user.container | tail -n +2 | cut -d: -f1 | sed 's#^Volume=/nix/store/##' | LC_ALL=C sort > /tmp/expected-user")
        machine.succeed(f"{user_podman} exec closure-user find /nix/store -mindepth 1 -maxdepth 1 -printf '%f\\n' | LC_ALL=C sort > /tmp/actual-user")
        machine.succeed("cmp /tmp/expected-user /tmp/actual-user")
        machine.succeed(f"{user_podman} exec closure-user cat /proc/mounts > /tmp/mounts-user")
        machine.succeed("awk -v expected=$(($(wc -l < /tmp/expected-user) + 1)) '$2 == \"/nix/store\" || index($2, \"/nix/store/\") == 1 { count++; if ((\",\" $4 \",\") !~ /,ro,/) exit 1 } END { if (count != expected) exit 1 }' /tmp/mounts-user")

    with subtest("missing closure source fails without fallback"):
        machine.succeed("systemctl stop closure-system.service")
        machine.succeed("mkdir -p /run/containers/systemd")
        machine.succeed("cp /etc/containers/systemd/closure-system.container /run/containers/systemd/closure-missing.container")
        machine.succeed("sed -i 's/ContainerName=closure-system/ContainerName=closure-missing/' /run/containers/systemd/closure-missing.container")
        machine.succeed("sed -i '0,/^Volume=\\/nix\\/store\\//s#^Volume=/nix/store/[^:]*#Volume=/nix/store/00000000000000000000000000000000-missing#' /run/containers/systemd/closure-missing.container")
        machine.succeed("systemctl daemon-reload")
        machine.fail("systemctl start closure-missing.service")
        machine.succeed("test $(systemctl show closure-missing.service -P Result) = exit-code")
  '';
}
