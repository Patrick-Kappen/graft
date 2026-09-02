# Issue #368 — rootless Quadlet readiness: investigation at baseline `8a9f2cf`

Internal diagnostic record for release blocker #368. Baseline SHA:
`8a9f2cf1ef2962f087c230ea64b2497c0d830f8c` (post-merge of PR #366 and PR #367).
Primary evidence: central workflow run
[`33416114327`](https://github.com/Patrick-Kappen/graft/actions/runs/33416114327)
dispatched with `runtime_tests=true` at that exact SHA.

Historical context: [#363](https://github.com/Patrick-Kappen/graft/issues/363)
(original `Result=protocol` notify race), #364 (design),
[#365](https://github.com/Patrick-Kappen/graft/issues/365) (`ExitType=cgroup`
decision), PR #366 (implementation), PR #367 (protocol-fixture repair).

## 1. What the failed central run actually shows

Run `33416114327` reported three failed jobs. They are **three different
phenomena**; only one of them is the flaky readiness race named in #368.

### 1.1 `activation-runtime` — stuck `activating` after "re-adding startup intent"

The subtest reactivates startup intent, reboots, and then waits for
`linger-user.service` to reach `active` or `failed`:

```
[ 53.10] systemd[1]: systemd-reboot.service: Deactivated successfully.
[ 54.38] reboot: machine restart
... boot ...
16:54:03 machine: waiting for success: systemctl --user show linger-user.service -P ActiveState
        case "$state" in active|failed) exit 0 ;; *) exit 1 ;; esac
16:56:04 !!! Test "re-adding startup intent takes effect on the next boot" failed
        with error: "action timed out after 120.61 seconds (timeout=120)"
```

During the whole 120 s window the guest console produced **only 8 dhcpcd
lines and zero user-manager output** — no `Starting`, no notify attribution,
no start timeout, no restart loop.

### 1.2 `notify-protocol-runtime` (normal) — same hang, on reboot 3 of 8

Reboots 1 and 2 passed with clean attribution; reboot 3:

```
[21.374] systemd[585]: Starting linger-user.service...
[22.065] podman[900]: container create 3e70... (linger-user)
[22.682] podman[900]: container start  3e70...
[22.712] linger-user[900]: 3e70b6e6...          ← podman -d frontend returns
... then 120.12 s of console silence ...
16:56:15 !!! wait_for_linger_result() → RequestedAssertionFailed:
        action timed out after 120.12 seconds (timeout=120)
```

Same signature as 1.1: the unit sits in **`activating`** — never `active`,
never `failed`. Compare a clean boot's journal:

```
linger-user.service: Got notification message from PID 934: MAINPID=984, READY=1
linger-user.service: New main PID 984 belongs to service, we are happy.
```

### 1.3 `notify-protocol-runtime` (debug) — deterministic, and *not* a race

The debug VM fails at the **very first protocol control**, before any
readiness loop:

```
machine: must succeed: ... systemctl --user start --no-block graft-exit-type-main.service
machine # Failed to start graft-exit-type-main.service: Unit graft-exit-type-main.service not found.
!!! RequestedAssertionFailed: command ... failed (exit code 5)
```

This is a broken test fixture, not a readiness failure (§2).

Both runtime jobs are declared `continue-on-error: true` (`ci.yml`), so the
overall run still reported `✓` while carrying three failed jobs — the
"advisory" pattern #368 explicitly rejects.

## 2. Root cause 1 (deterministic): fixture merge bug from PR #367

`tests/nixos/notify-protocol.nix` builds the node's `systemd` attrs as:

```nix
systemd = {
  user.services = { graft-exit-type-main.serviceConfig = ...; ... };
  tmpfiles.rules = [ ... ];
  services.graft-foreign = { ... };
}
// lib.optionalAttrs debugLogging {
  user.extraConfig = "LogLevel=debug";
};
```

`//` is a **shallow** merge. In the debug variant the right operand's `user`
attr **replaces** the entire left `user` attr, silently deleting
`user.services`. Consequences, all verified at the baseline:

- Both NixOS configs evaluated side by side:
  `systemd.user.services` keys = `[dbus dbus-broker graft-exit-type-cgroup
  graft-exit-type-cgroup-no-notify graft-exit-type-main nixos-activation
  podman systemd-tmpfiles-setup]` for normal, but **no `graft-exit-type-*`
  at all** for debug.
- Derivation closure of the two test drivers: the debug closure contains
  `graft-exit-type-protocol-parent.drv` (the script) but **none** of
  `unit-graft-exit-type-{main,cgroup,cgroup-no-notify}.service.drv`; the
  normal closure contains all three.
- The debug VM's user manager scans `/etc/systemd/user` (34 `Linked unit
  file:` debug lines) but never sees `graft-exit-type-*`; `systemctl --user
  start` → `Unit ... not found` (exit 5). Reproduced 1:1 locally at the
  baseline (same failure text, manager PID 634).

History: the `// optionalAttrs debugLogging` pattern predates #367 (added
with debug logging in `2a98781`); PR #367 (`8a9f2cf`) moved the
protocol-control units from `environment.etc` into `systemd.user.services`,
which is when the latent shallow-merge bug became fatal. Before #367 the
debug variant's units collided with the generated Quadlet path — PR #367
fixed the collision *and* introduced this deletion. (So #367's claimed
"fix" could not have worked for the debug variant even in isolation.)

**Fix on this branch** (`tests/nixos/notify-protocol.nix`): merge `extraConfig`
inside the `user` attr so `services` is preserved:

```nix
systemd.user = {
  services = { ... };
} // lib.optionalAttrs debugLogging { extraConfig = "LogLevel=debug"; };
```

Both variants now evaluate identically. The fixed debug VM passes the full
suite locally: 6 protocol controls (incl. `ExitType=main` →
`Result=protocol`, `ExitType=cgroup` → notify → `active`, and the no-notify
`protocol` case), 10 active-manager attempts, 8 reboots — with the debug
journal recording the attribution line on every boot. No assertion was
weakened.

## 3. Root cause 2 (flaky): `ExitType=cgroup` converts the notify race into an indefinite `activating`

The generated unit (reproduced with the pinned quadlet generator,
podman 5.8.2, from the exact `.container` file shipped in the VM) is:

```
[Service]
ExitType=cgroup
KillMode=mixed
Type=notify
NotifyAccess=all
ExecStart=podman run --name linger-user --replace --rm --cgroups=split
          --sdnotify=conmon -d ... /bin/graft-pause
[Install]
WantedBy=default.target
```

No `Restart=`, no `TimeoutStartSec=`.

Observed behavior across the central run:

| boot | notification | result |
|---|---|---|
| initial boot | `MAINPID=984, READY=1` attributed at 25.5 s | active/success |
| reboot 1 | attributed | active/success |
| reboot 2 | attributed | active/success |
| **reboot 3** | **never attributed, nothing logged** | **activating 120 s+, test aborts** |

Mechanism: with `ExitType=main` (pre-#366), the `podman run -d` frontend
exit is the main-process exit, and the missing/late `READY=1` produces the
old `Result=protocol` failure within seconds (the #363 race). With
`ExitType=cgroup`, the frontend exit no longer ends the unit; systemd keeps
the unit `activating` while the cgroup (conmon) is alive, and the 120 s
test poll sees neither `active` nor `failed`. **The underlying
attribution sensitivity identified in #363 is still present; only its
symptom changed — from a fast, visible `Result=protocol` to a silent,
indefinite hang that wedges the boot test.** (This is arguably worse for
the product: a wedged service instead of a failed one.)

The judge's list from #368 (no `READY=1` sent; sent but
rejected/unattributed; MAINPID/cgroup handoff; stuck `activating` vs
protocol failure) cannot be resolved from the failed run alone because the
current fixture kills the VM at the 120 s abort without dumping the user
manager's journal. With default `LogLevel=info`, *rejected* notifications
are logged at debug level — invisible in the normal VM. The debug variant,
whose debug journal would show the rejection reason, was disabled by the
§2 bug. That is why the two fixes/instrumentation in this branch are a
prerequisite for diagnosing the race.

Sub-question resolved during this investigation: the plain host user manager
(systemd 260.2, same version as the VM) fails a `Type=notify`+`ExitType=cgroup`
unit with `Result=timeout` at exactly its `DefaultTimeoutStartUSec` (90 s)
— both with the main process exited (child parked in the cgroup) and with a
live main PID. The test VMs, however, run **`DefaultTimeoutStartUSec=5min`**
because NixOS's test infrastructure (`test-instrumentation.nix`) injects
`systemd.user.extraConfig = "DefaultTimeoutStartSec=300 ..."` into every test
VM (`nixpkgs/nixos/modules/testing/test-instrumentation.nix:241`; verified in
-VM via the reproducer's `systemctl --user show -p DefaultTimeoutStartUSec`
→ `5min`, and `TimeoutStartUSec=5min` on the generated unit).

That closes the question: no timer weirdness is needed. The wedged unit
simply exceeds the 120 s test poll, and would fail `Result=timeout` only at
300 s (test VMs) or 90 s (production defaults).

**Re-framing for the design decision (#364):** `ExitType=cgroup` (PR #366) did
not remove the race identified in #363. It changed *how the race fails*:

- `ExitType=main` (pre-#366): frontend exits → `Result=protocol` within
  seconds, systemd kills the cgroup (fast, visible).
- `ExitType=cgroup` (post-#366): frontend exit is no longer an exit; a lost/
  unattributed notification leaves the service `activating` until
  `TimeoutStartSec` (90 s production / 300 s in test VMs), then
  `Result=timeout` — and the restart policy (if any) governs what happens
  next.

So the release-blocker symptom went from a fast `Result=protocol` to a slow
`Result=timeout` (with a 90–300 s wedge in between); on the central runner,
where the 120 s test poll aborts first, it shows up as the reported
"neither active nor failed". The attribution fix (#364) is still required;
`ExitType=cgroup` is a band-aid that re-typed and slowed the failure.

## 4. Why central reproduces while local passes

Local hands-off runs at the baseline passed 10/10 (`#368`) plus the runs in
this investigation (normal ×2, debug fixed ×1 all green); the central
runner failed on reboot 3/8 and in the activation reboot.

Structural differences, with the central side reproducing:

- CI runs `activation_runtime`, `notify_protocol_runtime`, `cdi_runtime`,
  plus `nix`, `rust`, `checks`, `docs` as **concurrent jobs** on
  `ubuntu-latest` runners (2-vCPU CPU-quota hosts sharing physical
  hardware), so the guest gets nested-KVM vCPU contention and steal time.
  The 120 s hang windows show *complete* console silence — consistent with
  the guest being starved around the critical handoff (container start →
  conmon notify → attribution) rather than with any Graft logic.
- The local host (16 cores, otherwise idle) gives the guest consistent CPU,
  and attribution lands inside the observed window every time.

Both are consistent with the #363 mechanism: the single
`MAINPID=…, READY=1` notification is racy against cgroup assignment /
attribution; when the guest is starved at the wrong moment, the message
lands where systemd cannot attribute it. This remains a hypothesis until a
debug-journal capture of a hang distinguishes
not-sent vs rejected/unattributed (§5).

## 5. Diagnostic work delivered on this branch

1. **Fixture fix** (§2) — the debug protocol-control units now reach the
   intended user manager; debug variant executes the protocol controls
   instead of failing at `Unit not found`.
2. **Bounded pre-teardown evidence** in `tests/nixos/notify-protocol.nix`
   (`wait_for_linger_result` + `record_diagnostics`): on a 120 s
   ready-timeout the test now captures, before teardown: `systemctl --user
   status/show` (incl. `NotifyAccess`, `TimeoutStartUSec`, `NRestarts`),
   process tree, `cgroup.procs` under the service, and the user journal
   grepped for notify/attribution/rejection lines (`MAINPID`, `READY=1`,
   `notification message`, `new main PID`, `belongs to unit`, `not in our
   cgroup`, `does not belong`, `reception only`).
3. **Minimal rootless reproducer**
   `tests/nixos/rootless-sdnotify-repro.nix` (+ `flake.nix` package
   `rootless-sdnotify-repro-test` / `-debug-test`), independent of Graft
   activation: a linger user + one Quadlet `.container`
   (`Type=notify`/`NotifyAccess=all`/`--sdnotify=conmon -d`/`ExitType=cgroup`
   driving a static `sleep`) rebooted N times with the same evidence
   capture, plus per-boot manager `DefaultTimeoutStartUSec` and unit
   `TimeoutStartUSec`. The VM normalizes `network-online.target`
   (Quadlet 5.8.2 unconditionally gates containers on the
   `podman-user-wait-network-online` wrapper, which only completes in the
   graft VMs because their rootful network containers bring the bridge up).

### 5b. Local reproduction results (this investigation)

| run | outcomes | result |
|---|---|---|
| notify-protocol normal (baseline, fresh) | 8 reboots | ✓ |
| notify-protocol debug (pre-fix) | `Unit graft-exit-type-main.service not found` | ✗ deterministic |
| notify-protocol debug (post-fix) | protocol controls + 10 active-manager + 8 reboots | ✓ |
| notify-protocol debug (4-VM concurrency) | 8 reboots, with activation+repro concurrent | ✓ |
| activation-runtime (4-VM concurrency) | ex-failing subtest 23.4 s | ✓ |
| reproducer normal / debug | 16 + 16 reboots | ✓ |
| CPU-pinned (1 core) starved: notify-debug | 8 reboots (245 s) | ✓ |
| CPU-pinned (1 core) starved: repro-debug | eval stalled (local cache lock) | inconclusive |

So far no local run has reproduced the hang — including two 2-vCPU VMs
pinned to a single core each. The central-runner CPU quota / host
oversubscription (nested KVM on quota-capped, oversubscribed runners)
remains the leading explanation for the central-vs-local gap, but it cannot
be fully emulated from this host; the reproducer + pre-teardown evidence
capture are the tools to run on the central runner to capture the
definitive journal if the hang recurs there. (The second starved
run never reached its VM: single-core evaluation stalled on the local nix
eval-cache lock, so it adds no signal.)

## 6. Status vs. acceptance criteria

| Criterion | Status |
|---|---|
| Debug variant executes its protocol controls | fixed + verified locally (central CI pending) |
| Central `runtime_tests=true`: activation-runtime, normal & debug notify-protocol pass | **not met** — hang still possible (§3) |
| ≥10 uninstrumented local runs on the final SHA | in progress |
| Independent reviewer GO | none |
| `v0.4.0-alpha.1` tag/prerelease absent | ✓ (nothing tagged) |

## 7. Open questions

1. In a hung boot, was `READY=1` never sent, sent-but-rejected, or
   sent-but-unattributed? (debug-journal capture via the fixed debug VM /
   reproducer under load).
2. Why does the VM's user manager not enforce `DefaultTimeoutStartUSec` in
   the hang (host probe fails at 90 s; VM silent for 120 s)?
3. Is CI-host CPU contention (4 concurrent VM jobs on quota-capped nested
   KVM) the trigger? (To be tested by running several VM instances
   concurrently locally.)
4. Any *safe* fail-fast bound (e.g. explicit `TimeoutStartSec`) only
   converts the hang into `Result=timeout`; it does not fix attribution.
   Per #364/#368 it must not be shipped as a masking workaround without an
   evidenced design decision.