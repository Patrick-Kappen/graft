{
  pkgs,
  graftPackage,
  debugLogging ? false,
}:

let
  inherit (pkgs) lib;

  commonRoot = ../nix/runtime-activation/common;
  enabledRoot = ../nix/runtime-activation/enabled;

  userEval = lib.evalModules {
    specialArgs = { inherit pkgs; };
    modules = [
      ./lib/hm-test-options.nix
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
  protocolParent = pkgs.writeShellScript "graft-exit-type-protocol-parent" ''
    set -euo pipefail
    control=$1

    printf '%s\n' "$BASHPID" > "$control/parent.pid"
    cat /proc/self/cgroup > "$control/parent.cgroup"
    (
      printf '%s\n' "$BASHPID" > "$control/child.pid"
      cat /proc/self/cgroup > "$control/child.cgroup"
      : > "$control/child-ready"
      printf '%s\n' ready > "$control/child-started"
      IFS= read -r action < "$control/release"
      case "$action" in
        notify)
          : > "$control/notified"
          ${pkgs.systemd}/bin/systemd-notify --pid="$BASHPID" --ready --status="Graft protocol child ready"
          exec ${pkgs.coreutils}/bin/sleep infinity
          ;;
        exit)
          exit 0
          ;;
        *)
          echo "unexpected protocol action: $action" >&2
          exit 1
          ;;
      esac
    ) &
    printf '%s\n' "$!" > "$control/child-shell.pid"
    IFS= read -r child_started < "$control/child-started"
    test "$child_started" = ready
    ${pkgs.diffutils}/bin/cmp "$control/parent.cgroup" "$control/child.cgroup"
    : > "$control/parent-returning"
  '';

in
{
  name = "graft-rootless-notify-protocol${lib.optionalString debugLogging "-debug"}";

  nodes.machine = { ... }: {
    imports = [ ../../modules/nixos.nix ];

    services.graft = {
      enable = true;
      package = graftPackage;
      hostId = "018f0f77-8c4d-7b2a-8e6a-4b8a7d3a1c20";
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
      "graft-exit-type-protocol-parent".source = protocolParent;
      "systemd/user/graft-exit-type-main.service".text = ''
        [Service]
        Type=notify
        NotifyAccess=all
        ExecStart=/etc/graft-exit-type-protocol-parent /run/user/%U/graft-exit-type-main
      '';
      "systemd/user/graft-exit-type-cgroup.service".text = ''
        [Service]
        Type=notify
        NotifyAccess=all
        ExitType=cgroup
        ExecStart=/etc/graft-exit-type-protocol-parent /run/user/%U/graft-exit-type-cgroup
      '';
      "systemd/user/graft-exit-type-cgroup-no-notify.service".text = ''
        [Service]
        Type=notify
        NotifyAccess=all
        ExitType=cgroup
        ExecStart=/etc/graft-exit-type-protocol-parent /run/user/%U/graft-exit-type-cgroup-no-notify
      '';
    };

    systemd = {
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
    }
    // lib.optionalAttrs debugLogging {
      user.extraConfig = "LogLevel=debug";
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
        exit_type = unit_property("ExitType")
        main_pid = unit_property("MainPID")
        control_group = unit_property("ControlGroup")
        observed = (
            f"{label}: state={state}/{substate} result={result} exit_type={exit_type} "
            f"main_pid={main_pid} control_group={control_group}"
        )
        print(observed)

        problems = []
        if (
            state != "active"
            or substate != "running"
            or result != "success"
            or exit_type != "cgroup"
        ):
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
        elif main_pid != "0":
            cgroup_status, cgroup_output = machine.execute(
                f"grep -Fx {main_pid} /sys/fs/cgroup{control_group}/cgroup.procs"
            )
            if cgroup_status != 0:
                problems.append(
                    f"{label}: main PID {main_pid} is absent from {control_group}: {cgroup_output}"
                )

        if problems:
            record_diagnostics(label)
        else:
            print_command(
                "journalctl -b _UID=1000 --no-pager -o short-monotonic | "
                "grep -Ei '(MAINPID|READY=1|notification message|new main PID|belongs to unit)' | "
                "tail -n 40"
            )
        return problems

    def protocol_control(unit):
        control = f"/run/user/1000/{unit}"
        machine.succeed(
            f"install -d -m 0700 -o 1000 -g 1000 {control}; "
            f"mkfifo -m 0600 {control}/child-started {control}/release"
        )
        return control

    def wait_for_protocol_child(control):
        machine.wait_for_file(f"{control}/child-ready", timeout=30)
        parent_pid = machine.succeed(f"cat {control}/parent.pid").strip()
        child_pid = machine.succeed(f"cat {control}/child.pid").strip()
        machine.wait_until_succeeds(f"test ! -e /proc/{parent_pid}", timeout=30)
        machine.succeed(f"cmp {control}/parent.cgroup {control}/child.cgroup")
        machine.fail(f"test -e {control}/notified")
        return child_pid

    def protocol_state(unit, expected_state):
        machine.wait_until_succeeds(
            user_systemctl(
                f"show {unit}.service -P ActiveState | grep -Fx {expected_state}"
            ),
            timeout=30,
        )
        return {
            property_name: machine.succeed(
                user_systemctl(
                    f"show {unit}.service -P {property_name}"
                )
            ).strip()
            for property_name in ["ActiveState", "SubState", "Result", "MainPID", "ControlGroup"]
        }

    def assert_no_service_cgroup(unit, state):
        assert state["MainPID"] == "0", f"{unit}: retained main PID {state['MainPID']}"
        control_group = state["ControlGroup"]
        if control_group:
            machine.wait_until_succeeds(
                f"test ! -e /sys/fs/cgroup{control_group}", timeout=30
            )

    def run_exit_type_protocol_controls():
        machine.succeed(
            "test $(systemctl --version | awk 'NR == 1 { print $2 }') -ge 250"
        )

        main_unit = "graft-exit-type-main"
        main_control = protocol_control(main_unit)
        machine.succeed(user_systemctl(f"start --no-block {main_unit}.service"))
        wait_for_protocol_child(main_control)
        main_state = protocol_state(main_unit, "failed")
        assert main_state["Result"] == "protocol", main_state
        assert_no_service_cgroup(main_unit, main_state)

        cgroup_unit = "graft-exit-type-cgroup"
        cgroup_control = protocol_control(cgroup_unit)
        machine.succeed(user_systemctl(f"start --no-block {cgroup_unit}.service"))
        cgroup_child = wait_for_protocol_child(cgroup_control)
        machine.succeed(f"test -e /proc/{cgroup_child}")
        activating = protocol_state(cgroup_unit, "activating")
        assert activating["Result"] == "success", activating
        machine.succeed(f"printf '%s\\n' notify > {cgroup_control}/release")
        active = protocol_state(cgroup_unit, "active")
        assert active["SubState"] == "running", active
        assert active["Result"] == "success", active
        assert active["MainPID"] == cgroup_child, active
        control_group = active["ControlGroup"]
        assert control_group.endswith("/app.slice/graft-exit-type-cgroup.service"), active
        machine.succeed(f"test -e /proc/{active['MainPID']}")
        machine.succeed(
            f"grep -Fx '0::{control_group}' /proc/{active['MainPID']}/cgroup"
        )
        machine.succeed(
            f"grep -Fx {active['MainPID']} /sys/fs/cgroup{control_group}/cgroup.procs"
        )
        machine.succeed(user_systemctl(f"stop {cgroup_unit}.service"))

        no_notify_unit = "graft-exit-type-cgroup-no-notify"
        no_notify_control = protocol_control(no_notify_unit)
        machine.succeed(user_systemctl(f"start --no-block {no_notify_unit}.service"))
        no_notify_child = wait_for_protocol_child(no_notify_control)
        machine.succeed(f"test -e /proc/{no_notify_child}")
        activating = protocol_state(no_notify_unit, "activating")
        assert activating["Result"] == "success", activating
        machine.succeed(f"printf '%s\\n' exit > {no_notify_control}/release")
        no_notify_state = protocol_state(no_notify_unit, "failed")
        assert no_notify_state["Result"] == "protocol", no_notify_state
        assert_no_service_cgroup(no_notify_unit, no_notify_state)

    machine.start(allow_reboot=True)
    wait_for_linger_result()

    failures = assess_active_unit("initial linger startup")
    machine.succeed(
        "grep -Fx 'ExitType=cgroup' /etc/containers/systemd/users/1000/linger-user.container"
    )
    run_exit_type_protocol_controls()
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
