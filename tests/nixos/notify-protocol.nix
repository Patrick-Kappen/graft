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

    systemd = {
      user.extraConfig = "LogLevel=debug";
      tmpfiles.rules = [
        "d /var/lib/graft-activation 0755 root root -"
        "d /var/lib/graft-workspace 0755 root root -"
        "f /var/lib/graft-workspace/marker 0644 root root - preserved"
      ];
      services.graft-foreign = {
        description = "Foreign unit preserved across Graft activation changes";
        wantedBy = [ "multi-user.target" ];
        serviceConfig = {
          Type = "oneshot";
          RemainAfterExit = true;
        };
        script = "touch /var/lib/graft-activation/foreign-unit";
      };
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

    def wait_for_linger_result():
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

    def assess_active_unit(label):
        state = unit_property("ActiveState")
        substate = unit_property("SubState")
        result = unit_property("Result")
        main_pid = unit_property("MainPID")
        control_group = unit_property("ControlGroup")
        observed = (
            f"{label}: state={state}/{substate} result={result} "
            f"main_pid={main_pid} control_group={control_group}"
        )
        print(observed)

        problems = []
        if state != "active" or substate != "running" or result != "success":
            problems.append(observed)
        if main_pid == "0":
            problems.append(f"{label}: no main PID")
        else:
            process_status, process_output = machine.execute(
                f"ps -p {main_pid} -o comm="
            )
            process_name = process_output.strip()
            print(
                f"{label}: main process exit={process_status} name={process_name}"
            )
            if process_status != 0 or process_name != "conmon":
                problems.append(
                    f"{label}: main process exit={process_status} name={process_name}, not conmon"
                )
        if not control_group.endswith("/app.slice/linger-user.service"):
            problems.append(f"{label}: unexpected control group {control_group}")

        if problems:
            record_diagnostics(label)
        else:
            print_command(
                "journalctl -b _UID=1000 --no-pager -o short-monotonic | "
                "grep -Ei '(MAINPID|READY=1|notification message|new main PID|belongs to unit)' | "
                "tail -n 40"
            )
        return problems

    machine.start(allow_reboot=True)
    wait_for_linger_result()

    failures = assess_active_unit("initial linger startup")
    machine.execute(user_systemctl("stop linger-user.service"))

    for attempt in range(1, 11):
        machine.succeed(user_systemctl("reset-failed linger-user.service"))
        start_status, _ = machine.execute(
            user_systemctl("start linger-user.service")
        )
        if start_status != 0:
            failures.append(f"active-manager-{attempt}: start exit {start_status}")
            record_diagnostics(f"failed active-manager attempt {attempt}")
        else:
            failures.extend(
                assess_active_unit(f"active-manager attempt {attempt}")
            )
        machine.execute(user_systemctl("stop linger-user.service"))

    for boot_attempt in range(1, 9):
        machine.reboot()
        wait_for_linger_result()
        failures.extend(
            assess_active_unit(f"linger bootstrap reboot {boot_attempt}")
        )

    if failures:
        raise Exception(
            "rootless Quadlet notify protocol failure(s): " + "; ".join(failures)
        )
  '';
}
