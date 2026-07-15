{ pkgs, graftPackage }:

let
  inherit (pkgs) lib;

  commonRoot = ../nix/runtime-activation/common;
  enabledRoot = ../nix/runtime-activation/enabled;
  disabledRoot = ../nix/runtime-activation/disabled;

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

  enabledUserEval = lib.evalModules {
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

  disabledUserEval = lib.evalModules {
    specialArgs = { inherit pkgs; };
    modules = [
      moduleTestOptions
      ../../modules/home-manager.nix
      {
        programs.graft = {
          enable = true;
          package = graftPackage;
          configRoot = commonRoot;
          configRoots = [ disabledRoot ];
        };
      }
    ];
  };

  enabledLingerUser =
    enabledUserEval.config.xdg.configFile."containers/systemd/linger-user.container".source;
  enabledLoginUser =
    enabledUserEval.config.xdg.configFile."containers/systemd/login-user.container".source;
  disabledLingerUser =
    disabledUserEval.config.xdg.configFile."containers/systemd/linger-user.container".source;
  disabledLoginUser =
    disabledUserEval.config.xdg.configFile."containers/systemd/login-user.container".source;

in
{
  name = "graft-startup-activation-runtime";

  nodes.machine =
    { lib, ... }:
    {
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

      environment.systemPackages = [
        pkgs.kexec-tools
        pkgs.util-linux
      ];

      environment.etc = {
        "containers/systemd/users/1000/linger-user.container".source = enabledLingerUser;
        "containers/systemd/users/1001/login-user.container".source = enabledLoginUser;
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

      specialisation.withoutActivation.configuration = {
        services.graft.configRoots = lib.mkForce [ disabledRoot ];
        environment.etc = {
          "containers/systemd/users/1000/linger-user.container".source = lib.mkForce disabledLingerUser;
          "containers/systemd/users/1001/login-user.container".source = lib.mkForce disabledLoginUser;
        };
      };
    };

  testScript =
    { nodes, ... }:
    let
      baseSystem = nodes.machine.system.build.toplevel;
      withoutActivation = "${baseSystem}/specialisation/withoutActivation";
    in
    ''
      def user_systemctl(uid, arguments):
          return (
              f"setpriv --reuid={uid} --regid=$(id -g {uid}) --clear-groups "
              f"env XDG_RUNTIME_DIR=/run/user/{uid} "
              f"DBUS_SESSION_BUS_ADDRESS=unix:path=/run/user/{uid}/bus "
              f"systemctl --user {arguments}"
          )

      def wait_for_user_unit(uid, unit):
          machine.wait_until_succeeds(f"test -d /run/user/{uid}", timeout=120)
          machine.wait_for_file(f"/run/user/{uid}/bus", timeout=120)
          show_state = user_systemctl(uid, f"show {unit} -P ActiveState")
          machine.wait_until_succeeds(
              f'state="$({show_state})"; '
              'case "$state" in active|failed) exit 0 ;; *) exit 1 ;; esac',
              timeout=120,
          )
          is_active = user_systemctl(uid, f"is-active {unit}")
          status = user_systemctl(uid, f"status {unit} --no-pager")
          machine.succeed(f"{is_active} || {{ {status}; exit 1; }}")

      machine.start(allow_reboot=True)
      machine.wait_for_unit("multi-user.target")

      with subtest("system startup lifecycles and dependency activation"):
          machine.wait_for_unit("long-running-system.service")
          machine.wait_for_unit("network-client-system.service")
          machine.wait_for_unit("network-owner-system.service")
          machine.wait_until_succeeds("test $(systemctl show startup-job-system.service -P Result) = success")
          machine.succeed("test $(systemctl show startup-job-system.service -P ActiveState) = inactive")
          machine.wait_until_succeeds("systemctl is-active setup-system.service")
          machine.succeed("test $(systemctl show setup-system.service -P SubState) = exited")
          machine.fail("systemctl is-active plain-system.service")
          machine.succeed("test $(wc -l < /var/lib/graft-activation/job-runs) -eq 1")
          machine.succeed("test $(wc -l < /var/lib/graft-activation/setup-runs) -eq 1")
          machine.succeed("systemctl is-active graft-foreign.service")
          machine.succeed("test -e /var/lib/graft-activation/foreign-unit")
          machine.succeed("grep -Fx preserved /var/lib/graft-workspace/marker")

      with subtest("linger starts the rootless user manager at boot"):
          machine.wait_for_file("/var/lib/systemd/linger/graftlinger")
          machine.wait_for_unit("user@1000.service")
          wait_for_user_unit(1000, "linger-user.service")
          machine.succeed(
              "runuser -u graftlinger -- podman info --format '{{.Host.Security.Rootless}}' | grep -Fx true"
          )

      with subtest("non-linger workload waits for login"):
          machine.fail("systemctl is-active user@1001.service")
          machine.send_key("alt-f2")
          machine.wait_until_succeeds("test $(fgconsole) = 2")
          machine.wait_for_unit("getty@tty2.service")
          machine.wait_until_tty_matches("2", "login: ")
          machine.send_chars("graftlogin\n")
          machine.wait_until_tty_matches("2", "Password: ")
          machine.send_chars("test\n")
          machine.wait_for_unit("user@1001.service")
          wait_for_user_unit(1001, "login-user.service")
          machine.send_chars("exit\n")
          machine.wait_until_fails("systemctl is-active user@1001.service", timeout=60)
          machine.wait_until_succeeds(
              "test $(systemctl show user@1001.service -P ActiveState) = inactive",
              timeout=120,
          )

      with subtest("live switch removes startup links without stopping workloads"):
          machine.succeed("${withoutActivation}/bin/switch-to-configuration switch")
          machine.succeed("test -f /etc/containers/systemd/long-running-system.container")
          machine.fail("grep -Fx 'WantedBy=multi-user.target' /etc/containers/systemd/long-running-system.container")
          machine.succeed("test -f /etc/containers/systemd/users/1000/linger-user.container")
          machine.fail(
              "grep -Fx 'WantedBy=default.target' /etc/containers/systemd/users/1000/linger-user.container"
          )
          machine.succeed("systemctl is-active long-running-system.service")
          machine.succeed(user_systemctl(1000, "is-active linger-user.service"))
          machine.succeed("test -e /var/lib/graft-activation/job-runs")
          machine.succeed("test -e /var/lib/graft-activation/setup-runs")
          machine.succeed("systemctl is-active graft-foreign.service")
          machine.succeed("test -e /var/lib/graft-activation/foreign-unit")
          machine.succeed("grep -Fx preserved /var/lib/graft-workspace/marker")
          job_runs_before_reboot = int(
              machine.succeed("wc -l < /var/lib/graft-activation/job-runs").strip()
          )
          setup_runs_before_reboot = int(
              machine.succeed("wc -l < /var/lib/graft-activation/setup-runs").strip()
          )

      with subtest("reboot honors removed startup intent"):
          machine.succeed(
              "kexec --load ${withoutActivation}/kernel "
              "--initrd ${withoutActivation}/initrd "
              ' --command-line "$(cat ${withoutActivation}/kernel-params) init=${withoutActivation}/init"'
          )
          machine.execute("systemctl kexec >&2 &", check_return=False)
          machine.connected = False
          machine.connect()
          machine.wait_for_unit("multi-user.target")
          machine.wait_for_unit("user@1000.service")
          wait_for_user_unit(1000, "default.target")
          machine.succeed("test -f /etc/containers/systemd/long-running-system.container")
          machine.fail("grep -Fx 'WantedBy=multi-user.target' /etc/containers/systemd/long-running-system.container")
          machine.succeed("test -f /etc/containers/systemd/users/1000/linger-user.container")
          machine.fail(
              "grep -Fx 'WantedBy=default.target' /etc/containers/systemd/users/1000/linger-user.container"
          )
          machine.fail("systemctl is-active long-running-system.service")
          machine.fail(user_systemctl(1000, "is-active linger-user.service"))
          machine.wait_for_unit("network-client-system.service")
          machine.wait_for_unit("network-owner-system.service")
          machine.wait_until_succeeds("test $(systemctl show startup-job-system.service -P Result) = success")
          job_runs_after_reboot = int(
              machine.succeed("wc -l < /var/lib/graft-activation/job-runs").strip()
          )
          setup_runs_after_reboot = int(
              machine.succeed("wc -l < /var/lib/graft-activation/setup-runs").strip()
          )
          assert job_runs_after_reboot == job_runs_before_reboot + 1
          assert setup_runs_after_reboot == setup_runs_before_reboot + 1
          machine.succeed("systemctl is-active graft-foreign.service")
          machine.succeed("test -e /var/lib/graft-activation/foreign-unit")
          machine.succeed("grep -Fx preserved /var/lib/graft-workspace/marker")

      with subtest("login does not override absent startup intent"):
          machine.fail("systemctl is-active user@1001.service")
          machine.send_key("alt-f2")
          machine.wait_until_succeeds("test $(fgconsole) = 2")
          machine.wait_until_tty_matches("2", "login: ")
          machine.send_chars("graftlogin\n")
          machine.wait_until_tty_matches("2", "Password: ")
          machine.send_chars("test\n")
          machine.wait_for_unit("user@1001.service")
          wait_for_user_unit(1001, "default.target")
          machine.fail(user_systemctl(1001, "is-active login-user.service"))
          machine.send_chars("exit\n")
          machine.wait_until_fails("systemctl is-active user@1001.service", timeout=60)
          machine.wait_until_succeeds(
              "test $(systemctl show user@1001.service -P ActiveState) = inactive",
              timeout=120,
          )

      with subtest("re-adding startup intent takes effect on the next boot"):
          machine.succeed("${baseSystem}/bin/switch-to-configuration test")
          machine.succeed("grep -Fx 'WantedBy=multi-user.target' /etc/containers/systemd/long-running-system.container")
          machine.succeed(
              "grep -Fx 'WantedBy=default.target' /etc/containers/systemd/users/1000/linger-user.container"
          )
          job_runs_before_readd_reboot = int(
              machine.succeed("wc -l < /var/lib/graft-activation/job-runs").strip()
          )
          setup_runs_before_readd_reboot = int(
              machine.succeed("wc -l < /var/lib/graft-activation/setup-runs").strip()
          )
          machine.reboot()
          machine.wait_for_unit("multi-user.target")
          machine.wait_for_unit("long-running-system.service")
          machine.wait_for_unit("user@1000.service")
          wait_for_user_unit(1000, "linger-user.service")
          job_runs_after_readd_reboot = int(
              machine.succeed("wc -l < /var/lib/graft-activation/job-runs").strip()
          )
          setup_runs_after_readd_reboot = int(
              machine.succeed("wc -l < /var/lib/graft-activation/setup-runs").strip()
          )
          assert job_runs_after_readd_reboot == job_runs_before_readd_reboot + 1
          assert setup_runs_after_readd_reboot == setup_runs_before_readd_reboot + 1
          machine.succeed("systemctl is-active graft-foreign.service")
          machine.succeed("test -e /var/lib/graft-activation/foreign-unit")
          machine.succeed("grep -Fx preserved /var/lib/graft-workspace/marker")
    '';
}
