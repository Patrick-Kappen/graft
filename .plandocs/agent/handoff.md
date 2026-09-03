# Graft orchestration handoff — v0.4 release blocker

For the reusable operating procedure, see [the orchestrator playbook](orchestrator.md).
This handoff remains the authority for this release's current task and gates.

## Authority and operational rules

- Repository: `~/graft`, `Patrick-Kappen/graft`.
- Use separate tmux session `graft-v04-pi`; one worktree/window per Pi agent.
- Watch `~/.cache/pi-orch/events.jsonl` as the only agent-completion signal.
- Agents never push, open PRs, merge, tag, or publish unless Patrick explicitly authorizes it.
- Implementation and verification settlements pass daemon-owned automatic scans first. Red returns privately to the same subagent; green creates a fresh read-only Sol/medium review runner with only a bounded acceptance/diff/evidence bundle. Review never runs in the accumulated interactive orchestrator context, and raw lint output stays out of the maintainer UI.
- Do not make a release decision from local retries. Exact-SHA central runtime CI and an independent GO are mandatory.

## Release status

**NO-GO.** `v0.4.0-alpha.1` must not be tagged or published.

| Item | Value |
|---|---|
| Exact `origin/main` | `8a9f2cf1ef2962f087c230ea64b2497c0d830f8c` |
| Latest merges | #366 (`ExitType=cgroup`), #367 (notify protocol test fixture) |
| Release blocker | #368 — `fix(runtime): make rootless Quadlet readiness deterministic on CI` |
| Failed exact-SHA CI | [run 33416114327](https://github.com/Patrick-Kappen/graft/actions/runs/33416114327), dispatched with `runtime_tests=true` |

The central run failed `activation-runtime`, normal `notify-protocol-runtime`, and debug `notify-protocol-runtime`. The normal and activation failures time out waiting for rootless `linger-user.service` to become `active|failed`; debug cannot find its protocol-control user unit. Ten local normal uninstrumented VM runs passed, but do not supersede central evidence.

## Only authorized next task — #368 diagnostic phase

### Assigned subagent

| Setting | Value |
|---|---|
| Model | `opencode-go / deepseek-v4-flash / medium` |
| Worktree | `~/graft-v04-runtime-diagnostic` |
| Branch | `investigate/368-rootless-quadlet-readiness` |
| tmux window | `graft-v04-pi:runtime-diagnostic` |
| Session | `graft-v04-runtime-diagnostic` |
| Baseline | `8a9f2cf1ef2962f087c230ea64b2497c0d830f8c` |

Create the worktree from the baseline, start Pi with the `orch-status` extension and the session id above, then send this single prompt:

```text
Investigate GitHub issue #368 at exact baseline
8a9f2cf1ef2962f087c230ea64b2497c0d830f8c. Read #363–#365, PRs #366/#367,
and failed central run 33416114327 first.

Diagnostic phase only: do not modify tracked files, commit, push, open a PR,
tag, publish, weaken assertions, or alter the service protocol. Run only
focused diagnostic evaluation/VM commands needed to establish: (1) why central
normal notify-protocol and activation tests time out while local normal runs
pass; (2) why debug cannot find its user protocol unit; (3) whether READY=1 is
absent, late, rejected/unattributed, or leads to unacceptable MAINPID/cgroup
state; (4) a minimal rootless reproducer independent of Graft activation; and
(5) one minimal remediation plan with exact files, tests, and risks.

Preserve evidence, identify the SHA for every observation, and separate facts
from hypotheses. Do not make a new product-policy decision. End exactly:
STATE: done|blocked|failed — <one-sentence summary>.
```

## Gate after completion

The orchestrator reviews the diagnostic report and presents it to Patrick. No implementation is authorized until Patrick approves the remediation plan. The approved change is then implemented in a new worktree/session by Luna/medium with a complete, precise implementation prompt; the implementer does not reopen the design.

After any approved fix and merge: require ten independent focused rootless VM runs, then central `runtime_tests=true` CI on the exact final SHA with activation, normal+debug notify-protocol, and CDI runtime jobs all genuinely executed and passing, then independent GO. Only then tag/publish.
