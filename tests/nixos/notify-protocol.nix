{ pkgs, graftPackage }:

let
  inherit (pkgs) lib;

  commonRoot = ../nix/runtime-activation/common;
  enabledRoot = ../nix/runtime-activation/enabled;

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
          configRoot = commonRoot;
          configRoots = [ enabledRoot ];
        };
      }
    ];
  };

  lingerUser = userEval.config.xdg.configFile."containers/systemd/linger-user.container".source;
  loginUser = userEval.config.xdg.configFile."containers/systemd/login-user.container".source;

in
{
  name = "graft-rootless-notify-protocol";

  nodes.machine = { ... }: {
    imports = [ ../../modules/nixos.nix ];

    services.graft = {
      enable = true;
      package = graftPackage;
      configRoot = commonRoot;
      configRoots = [ enabledRoot ];
    };

    virtualisation = {
      diskSize = 4096;
      memorySize = 2048;
      podman.enable = true;
    };

    users.mutableUsers = false;
    users.users = {
      graftlinger = {
        isNormalUser = true;
        uid = 1000;
        linger = true;
      };
      graftlogin = {
        isNormalUser = true;
        uid = 1001;
        linger = false;
        initialPassword = "test";
      };
    };

    environment.systemPackages = [ pkgs.util-linux ];
    environment.etc = {
      "containers/systemd/users/1000/linger-user.container".source = lingerUser;
      "containers/systemd/users/1001/login-user.container".source = loginUser;
    };

    systemd.tmpfiles.rules = [
      "d /var/lib/graft-activation 0755 root root -"
      "d /var/lib/graft-workspace 0755 root root -"
      "f /var/lib/graft-workspace/marker 0644 root root - preserved"
    ];
    systemd.services.graft-foreign = {
      description = "Foreign unit preserved across Graft activation changes";
      wantedBy = [ "multi-user.target" ];
      serviceConfig = {
        Type = "oneshot";
        RemainAfterExit = true;
      };
      script = "touch /var/lib/graft-activation/foreign-unit";
    };
  };

  testScript = ''
    def user_command(arguments):
        return (
            "setpriv --reuid=1000 --regid=$(id -g 1000) --clear-groups "
            "env XDG_RUNTIME_DIR=/run/user/1000 "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus "
            f"{arguments}"
        )

    def user_systemctl(arguments):
        return user_command(f"systemctl --user {arguments}")

    def unit_property(property_name):
        return machine.succeed(
            user_systemctl(f"show linger-user.service -P {property_name}")
        ).strip()

    def print_command(command):
        status, output = machine.execute(command)
        print(f"diagnostic command exit={status}: {command}\n{output}")

    def record_diagnostics(label):
        print(f"notify-protocol diagnostics: {label}")
        print_command(
            user_systemctl("status linger-user.service --no-pager --full")
        )
        print_command(
            user_systemctl(
                "show linger-user.service "
                "-P ActiveState -P SubState -P Result -P MainPID -P ControlGroup"
            )
        )
        print_command(
            "ps -eo pid,ppid,uid,cgroup,args | "
            "grep -E '(PID|linger-user|conmon|graft-pause|passt)'"
        )
        print_command(
            "journalctl -b _UID=1000 --no-pager -o short-monotonic | "
            "grep -Ei '(linger-user|notify|protocol|conmon)' | tail -n 120"
        )

    machine.start()
    machine.wait_for_unit("multi-user.target")
    machine.wait_for_file("/var/lib/systemd/linger/graftlinger")
    machine.wait_for_unit("user@1000.service")
    machine.wait_until_succeeds("test -d /run/user/1000", timeout=120)
    machine.wait_for_file("/run/user/1000/bus", timeout=120)
    machine.wait_until_succeeds(
        user_systemctl(
            "show linger-user.service -P ActiveState | grep -Ex '(active|failed)'"
        ),
        timeout=120,
    )

    failures = []
    initial_state = unit_property("ActiveState")
    initial_result = unit_property("Result")
    record_diagnostics("initial linger startup")
    if initial_state != "active":
        failures.append(f"initial:{initial_state}/{initial_result}")

    machine.execute(user_systemctl("stop linger-user.service"))

    for attempt in range(1, 11):
        machine.succeed(user_systemctl("reset-failed linger-user.service"))
        start_status, _ = machine.execute(
            user_systemctl("start linger-user.service")
        )
        state = unit_property("ActiveState")
        result = unit_property("Result")
        if start_status != 0 or state != "active":
            record_diagnostics(f"failed active-manager attempt {attempt}")
            failures.append(
                f"attempt-{attempt}:exit-{start_status}/{state}/{result}"
            )
        else:
            main_pid = unit_property("MainPID")
            control_group = unit_property("ControlGroup")
            print(
                f"active-manager attempt {attempt}: "
                f"state={state} result={result} main_pid={main_pid} "
                f"control_group={control_group}"
            )
        machine.execute(user_systemctl("stop linger-user.service"))

    if failures:
        raise Exception(
            "rootless Quadlet notify protocol failure(s): " + ", ".join(failures)
        )
  '';
}
