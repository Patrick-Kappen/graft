# Issue #368 — local stress screening step 1 (control vs 1-vCPU guest)

## Scope & provenance

- Baseline SHA: `8a9f2cf1ef2962f087c230ea64b2497c0d830f8c` (detached worktree
  `/home/zerodawn/graft-v04-stress-repro`).
- TEST-ONLY changes only: `tests/nixos/stress-repro.nix` + two flake attrs
  (`rootless-sdnotify-stress-control-test`, `rootless-sdnotify-stress-1vcpu-test`).
  No product modules, existing tests, protocol code, commits, refs, push, or CI.
- Reproducer derived verbatim (protocol + evidence capture) from the prior
  diagnostic prototype
  `tests/nixos/rootless-sdnotify-repro.nix (branch investigate/368-rootless-quadlet-readiness)`;
  the only module-level addition is a `cores` parameter used by the stress entry.
- Baseline protocol preserved exactly for both runs: rootless linger user
  (uid 1000), one Quadlet `.container` unit (`Type=notify`,
  `NotifyAccess=all`, `--sdnotify=conmon -d`, `ExitType=cgroup`, static
  `sleep`), user manager from `user@1000.service`, initial boot + 16 reboot
  cycles, and on a 120 s readiness timeout a pre-teardown dump of
  `systemctl --user status/show`, user journal (notify/attribution/rejection
  keywords), `cgroup.procs`, and the process tree.

## Identities (both runs)

- Test derivation: `/nix/store/yi5fcy79gq9k2arry8klac34pqqpdvzb-vm-test-run-graft-rootless-sdnotify-repro.drv`
  (identical for control and 1-vCPU entries — `virtualisation.cores` evaluates
  to the nixpkgs default `1` in both, verified from the pinned nixpkgs rev
  `a50de1b7d8a586adc18d2395c19de7d6058e6030`; `qemu-vm.nix` `-smp
  ${config.virtualisation.cores}`).
- Driver binary: `/nix/store/ink6vmk7px1ijziq2mjr9fh9kcq5yv3v-nixos-test-driver-graft-rootless-sdnotify-repro/bin/nixos-test-driver`
  (config `/nix/store/901bpcn9y5smhvhh6x4qsqd3bqvk5x0r-driverConfiguration.json`)
- VM start script: `/nix/store/5m3vyxhy9i49p2hfkn4wv9xr71armxcb-nixos-vm/bin/run-machine-vm`
  → `qemu-system-x86_64 -machine accel=kvm:tcg -cpu max -m 2048 -smp 1`.
- Guest boot line in both: `smpboot: Brought up 1 node, 1 CPU`,
  `Total of 1 processors activated`.
- Host: AMD Ryzen 7 7700X (16 cores), `/dev/kvm` present, idle (load ~1),
  nix-daemon active (builds shared via daemon).
- `virtualisation.cores` already defaults to 1 for NixOS test VMs in the
  pinned nixpkgs; the declared stress — a 1-vCPU guest configuration — is
  therefore operative only as the host-side confinement of that single vCPU
  (no second variable: same protocol, same derivation, same qemu line).

## Run 1 — CONTROL (exact baseline protocol, unpinned)

Command:

```
cd /home/zerodawn/graft-v04-stress-repro
nohup env PYTHONUNBUFFERED=1 \
  /nix/store/ink6vmk7px1ijziq2mjr9fh9kcq5yv3v-nixos-test-driver-graft-rootless-sdnotify-repro/bin/nixos-test-driver \
  -o /home/zerodawn/graft-v04-stress-repro/run-control-result \
  > /home/zerodawn/graft-v04-stress-repro/run-control.log 2>&1
```

(launched cleanly at 23:00 after killing an earlier buffered-log attempt and
orphan qemu; log covers only the clean run).

Result — **FAILED at reboot 3 of 16** (exact per-boot assessments, from
`run-control.log`):

```
initial boot: state=active/running result=success main_pid=924
reboot 1:     state=active/running result=success main_pid=874
reboot 2:     state=active/running result=success main_pid=882
reboot 3:     linger-probe.service neither active nor failed after 120s  →  Exception
```

The readiness timeout hit the 120 s poll **after which the unit was still
`activating`**. Bounded pre-teardown evidence captured (same source SHA, same
config, same command):

- `systemctl --user status`: `Active: activating (start)` since 2min 0s at
  capture; `Process: 843 ExecStart=podman run ... --sdnotify=conmon -d ...
  (code=exited, status=0/SUCCESS)`; `Main PID: 843 (code=exited,
  status=0/SUCCESS)`; `Tasks: 3`; CGroup shows `libpod-payload-*` (sleep 905),
  `runtime` (pasta 890, conmon 898).
- `systemctl --user show`: `ActiveState=activating SubState=start Result=all
  MainPID=843 ExecMainPID=843 NotifyAccess=all TimeoutStartUSec=5min
  NRestarts=0 ControlGroup=.../linger-probe.service`.
- Process tree: conmon(898) child of user manager 592, sleep(905) child of
  conmon, both under the service cgroup; podman frontend 843 exited 0.
- `cgroup.procs`: top-level service cgroup empty (all PIDs live in the
  `libpod-payload-*` / `runtime` sub-cgroups from `--cgroups=split`);
  sub-cgroup dirs not named `*linger-probe*` so not individually dumped.
- User journal (grep for `MAINPID|READY=1|notification message|new main PID|
  belongs to unit|not in our cgroup|does not belong|reception only|sent.*READY|
  sd_notify|conmon|protocol|timeout|restart`): only `Starting
  linger-probe.service`, podman create/init/start, frontend output. **No
  notification/attribution lines at all.** (Note: the passing boots 1–2 also
  show no attribution lines at this LogLevel=info — grep exit=1 on all three
  passing per-boot greps — so journal silence alone does not distinguish
  pass/fail; the outcome does: `active` vs `activating > 120 s`.)

## Run 2 — STRESS (single declared variable: 1-vCPU guest configuration, host-confined)

Command:

```
cd /home/zerodawn/graft-v04-stress-repro
nohup env PYTHONUNBUFFERED=1 taskset -c 15 \
  /nix/store/ink6vmk7px1ijziq2mjr9fh9kcq5yv3v-nixos-test-driver-graft-rootless-sdnotify-repro/bin/nixos-test-driver \
  -o /home/zerodawn/graft-v04-stress-repro/run-1vcpu-result \
  > /home/zerodawn/graft-v04-stress-repro/run-1vcpu.log 2>&1
```

Verified during the run: driver and qemu `taskset -pc` affinity `15`;
qemu `-smp 1`; guest `Total of 1 processors activated`. Same derivation,
same protocol, same 16-reboot schedule.

Result — **PASSED**: `initial boot` + `reboot 1..16` all
`state=active/running result=success` (MainPIDs 912 … varied per boot); no
`activating` at any poll; `test script finished in 516.72s`. Evidence:
`run-1vcpu.log`.

## Exact comparison

| metric | control (unpinned, baseline protocol) | stress (1-vCPU guest, pinned to host core 15) |
|---|---|---|
| protocol | identical (Type=notify/NotifyAccess=all/--sdnotify=conmon -d/ExitType=cgroup, linger user, Quadlet) | identical |
| qemu/guest | `-smp 1`, 1 processor | `-smp 1`, 1 processor, host affinity=15 |
| initial boot | active/running/success | active/running/success |
| reboots | 1–2 active/success; **3 → activating > 120 s → FAIL** | 1–16 active/success |
| `activating` beyond 120 s | **YES (reboot 3)** | **NO** |
| evidence dump | captured (systemctl show/status, journal, cgroup.procs, process tree) | not needed (no timeout) |

## Findings

- The CONTROLLED baseline protocol reproduced the #368 wedge locally at this
  SHA: a service stuck in `activating` past 120 s on a reboot-cycle boot,
  with the notify frontend already exited 0, conmon and the container alive
  in the cgroup, no notification/attribution visible in the info-level user
  journal, and `TimeoutStartUSec=5min` (the failure would only surface as
  `Result=timeout` at 5 min — beyond the test poll). This matches the
  central-runner signature (fail on a later reboot, silent `activating`).
- The 1-vCPU-guest stress run (host-confined) passed all 17 cycles.
- Sample size is n=1 per configuration for an intermittent phenomenon; the
  two outcomes are reported exactly as observed. No protocol was changed and
  no additional variants were run (per instructions).

Artifacts (uncommitted, worktree-local): `tests/nixos/stress-repro.nix`,
`flake.nix` (2 test attrs), `run-control.log`, `run-1vcpu.log`,
`run-control-result/`, `run-1vcpu-result/`.

STATE: done — at baseline 8a9f2cf the exact-protocol control reproduced the
#368 `activating`-beyond-120s wedge locally on reboot 3 of 16 (evidence
captured pre-teardown), while the single declared stress variable, a
1-vCPU guest configuration host-confined to one core, passed all 17 cycles.