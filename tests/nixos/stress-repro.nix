# Minimal rootless sd_notify readiness reproducer — local stress screening for issue #368.
#
# TEST-ONLY. Derived verbatim (protocol + evidence capture) from the prior
# diagnostic prototype:
#   tests/nixos/rootless-sdnotify-repro.nix (branch investigate/368-rootless-quadlet-readiness)
# The control run must preserve the exact baseline protocol at SHA
# 8a9f2cf1ef2962f087c230ea64b2497c0d830f8c: rootless linger user, single
# Quadlet .container unit (Type=notify / NotifyAccess=all / --sdnotify=conmon
# -d / ExitType=cgroup) that must reach `active` on every boot.
#
# The ONLY deliberate deviation between the two screening runs is the guest
# CPU configuration, via `cores`:
#   - control: cores = null  -> the exact baseline machine config (NixOS test
#     VM default: virtualisation.cores = 1, guest scheduled anywhere on the
#     idle host).
#   - stress  : cores = 1    -> the single declared stress variable, a 1-vCPU
#     guest configuration, run host-confined to one physical core.
#
# Both variants assess the initial boot plus ${builtins.toString reboots}
# reboot cycles (default 16). On a readiness timeout it collects the bounded
# evidence required by #368 *before* teardown: systemctl --user status/show
# (incl. NotifyAccess / TimeoutStartUSec / NRestarts), user journal filtered
# for notify/attribution/rejection lines, cgroup.procs under the service,
# and the process tree — so the failure mode can be distinguished
# (no READY=1 sent; sent but rejected/unattributed; MAINPID/cgroup handoff;
# stuck `activating` vs terminal protocol failure).
#
# The container is deliberately a plain static `sleep` so nothing but the
# user manager + Podman/conmon notify path can influence readiness.
{
  pkgs,
  reboots ? 16,
  debugLogging ? false,
  cores ? null,
}:

let
  probeRootfs = pkgs.runCommand "linger-probe-rootfs" { } ''
    mkdir -p $out/bin
    cp ${pkgs.pkgsStatic.busybox}/bin/sleep $out/bin/sleep
    chmod 0755 $out/bin/sleep
  '';

  probeContainer = pkgs.writeText "linger-probe.container" ''
    [Container]
    ContainerName=linger-probe
    Rootfs=${probeRootfs}:O
    Exec=/bin/sleep 100000000

    [Service]
    ExitType=cgroup

    [Install]
    WantedBy=default.target
  '';
in
{
  name = "graft-rootless-sdnotify-repro${if debugLogging then "-debug" else ""}";

  nodes.machine =
    { lib, ... }:
    {
      virtualisation = {
        diskSize = 4096;
        memorySize = 2048;
        podman.enable = true;
      }
      // lib.optionalAttrs (cores != null) {
        # THE single stress variable for this screening step: the guest is
        # explicitly configured as a 1-vCPU guest. (NixOS test VMs default to
        # virtualisation.cores = 1; the host-side confinement that makes this
        # configuration operative is applied at invocation time, not here.)
        inherit cores;
      };

      users.mutableUsers = false;
      users.users.probe = {
        isNormalUser = true;
        uid = 1000;
        linger = true;
      };

      environment.etc = {
        "containers/systemd/users/1000/linger-probe.container".source = probeContainer;
      }
      // (
        if debugLogging then
          {
            "systemd/user.conf".text = "[Manager]\nLogLevel=debug\n";
          }
        else
          { }
      );

      # Quadlet 5.8.2 unconditionally adds
      # Wants/After=podman-user-wait-network-online.service to container
      # units; that wrapper polls `network-online.target`. In the graft VMs it
      # completes because the rootful system containers bring the bridge up;
      # this minimal VM has no such containers, so normalize the target
      # instead (it has no dependencies here, so it activates immediately at
      # boot).
      systemd.targets.network-online.wantedBy = [ "multi-user.target" ];
    };

  testScript = ''
    import time

    def user_command(arguments):
        return (
            "setpriv --reuid=1000 --regid=$(id -g 1000) --clear-groups "
            "env XDG_RUNTIME_DIR=/run/user/1000 "
            "DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/1000/bus "
            f"{arguments}"
        )

    def user_systemctl(arguments):
        return user_command(f"systemctl --user {arguments}")

    def print_command(command):
        status, output = machine.execute(command)
        print(f"diag exit={status}: {command}\n{output}")

    def collect_evidence(label):
        print(f"ROOTLESS-REPRO evidence: {label}")
        print_command(user_systemctl("status linger-probe.service --no-pager --full"))
        print_command(
            user_systemctl(
                "show linger-probe.service "
                "-P ActiveState -P SubState -P Result -P MainPID -P ControlGroup "
                "-P NotifyAccess -P TimeoutStartUSec -P NRestarts -P ExecMainPID"
            )
        )
        print_command(
            "ps -eo pid,ppid,uid,cgroup,args | "
            "grep -E '(PID|linger-probe|conmon|catatonit|sleep)'"
        )
        print_command(
            "find /sys/fs/cgroup -type d -name '*linger-probe*' "
            "-exec sh -c 'echo {}; cat {}/cgroup.procs' \\;"
        )
        print_command(
            "journalctl -b _UID=1000 --no-pager -o short-monotonic | "
            "grep -Ei "
            "'(linger-probe|MAINPID|READY=1|notification message|new main PID|belongs to unit|"
            "not in our cgroup|does not belong|reception only|sent.*READY|sd_notify|conmon|"
            "protocol|timeout|restart)' | tail -n 200"
        )

    def wait_for_probe_result():
        machine.wait_for_unit("multi-user.target")
        machine.wait_for_file("/var/lib/systemd/linger/probe")
        machine.wait_for_unit("user@1000.service")
        machine.wait_until_succeeds("test -d /run/user/1000", timeout=120)
        machine.wait_for_file("/run/user/1000/bus", timeout=120)
        deadline = time.time() + 120
        while True:
            status, output = machine.execute(
                user_systemctl("show linger-probe.service -P ActiveState")
            )
            if output.strip() in ("active", "failed"):
                return
            if time.time() >= deadline:
                collect_evidence(
                    "linger-probe.service neither active nor failed after 120s"
                )
                raise Exception(
                    "linger-probe.service did not reach active or failed within 120s; "
                    "evidence above"
                )
            time.sleep(2)

    def assess_boot(label):
        print_command(
            "cat /etc/systemd/user.conf 2>/dev/null || true"
        )
        print(
            "user-manager defaults: "
            + machine.succeed(
                user_systemctl(
                    "show -p DefaultTimeoutStartUSec -p DefaultRestartUSec"
                )
            ).strip()
        )
        state = machine.succeed(
            user_systemctl("show linger-probe.service -P ActiveState")
        ).strip()
        substate = machine.succeed(
            user_systemctl("show linger-probe.service -P SubState")
        ).strip()
        result = machine.succeed(
            user_systemctl("show linger-probe.service -P Result")
        ).strip()
        main_pid = machine.succeed(
            user_systemctl("show linger-probe.service -P MainPID")
        ).strip()
        control_group = machine.succeed(
            user_systemctl("show linger-probe.service -P ControlGroup")
        ).strip()
        print(
            f"{label}: state={state}/{substate} result={result} "
            f"main_pid={main_pid} control_group={control_group}"
        )
        timeout_start = machine.succeed(
            user_systemctl("show linger-probe.service -P TimeoutStartUSec")
        ).strip()
        print(f"{label}: TimeoutStartUSec={timeout_start}")
        problems = []
        if state != "active" or substate != "running" or result != "success":
            problems.append(f"{label}: unexpected state {state}/{substate} result={result}")
        if main_pid == "0":
            problems.append(f"{label}: no main PID")
        print_command(
            "journalctl -b _UID=1000 --no-pager -o short-monotonic | "
            "grep -Ei '(MAINPID|READY=1|notification message|new main PID|belongs to unit)' | "
            "tail -n 20"
        )
        return problems

    machine.start(allow_reboot=True)
    wait_for_probe_result()
    failures = assess_boot("initial boot")

    for attempt in range(1, ${toString reboots} + 1):
        machine.reboot()
        wait_for_probe_result()
        failures.extend(assess_boot(f"reboot {attempt}"))

    if failures:
        collect_evidence("reproducer failures detected")
        raise Exception(
            "rootless sdnotify readiness failure(s): " + "; ".join(failures)
        )
  '';
}