# Graft orchestrator implementation status

## Architecture

The orchestrator is split into three roles:

1. **Durable daemon** (`daemon.mjs`) owns queue state, GitHub admission refresh,
   worktrees, tmux runners, live activity, settlements, review ordering,
   decisions, and crash recovery. It remains active in
   `graft-v04-pi:orch-daemon` when the Pi frontend closes.
2. **Pi frontend** (`index.ts`, `dashboard.ts`) subscribes to daemon
   snapshots/events. Its persistent widget shows live counts, running-agent
   activity, and the maintainer's pending-action count. `/orch` opens a
   coloured overlay dashboard whose default tab contains only work requiring
   maintainer action; separate tabs expose active and historical tasks. Detail,
   decision, admission, and explanation flows use modal overlays with
   **Yes / No / Explain** controls.
3. **Automatic preflight** runs after implementation or verification settles.
   It mirrors the applicable local required-CI scans in a quick phase followed
   by the expensive full phase. A red phase is returned privately to the same
   logical subagent in a fresh bounded repair session; raw lint output stays in
   owner-only scan logs and never creates a normal review turn. Green creates a
   separate fresh review task. After three consecutive private repair attempts,
   a persistent infrastructure failure is surfaced as an exceptional item
   rather than looping without bound.
4. **Fresh review agent** runs as a new read-only Sol/medium session. The daemon
   gives it only a generated bounded acceptance/diff/evidence bundle and changed
   files—never prior Pi sessions or the interactive orchestrator conversation.
   Its typed terminal verdict automatically approves, privately returns concrete
   findings, or creates a maintainer decision.

A maintainer can admit one bounded **ordered pipeline** instead of repeatedly
admitting each internal phase. The daemon persists its plan and task
relationships; accepted implementation automatically unlocks independent local
verification. Completion of local verification creates the first genuine
external-action gate (for example central `runtime_tests=true` dispatch).

Subagents run as `pi --mode rpc` processes inside one tmux window per active
turn. `runner.mjs` keeps RPC stdout private, mirrors raw JSON to the pane, and
streams bounded live events to the daemon over an owner-only Unix socket.
Settled windows close only after the durable event is appended. Implementation
repair follow-ups start a **fresh Pi session** with a bounded handoff. Semantic
review always starts a different session and receives no prior-result narrative
or evidence-session path for implementation work; it receives the generated
baseline change bundle instead. Logical agent identity, model,
worktree, and task stay unchanged. Model and thinking routes are explicit and
durable: Terra/medium for design, parallel DeepSeek Flash/low shards for
routine implementation, Luna/low for green-shard integration, DeepSeek
Flash/low for independent verification, Luna/high for exceptional high-risk
implementation, and low thinking for exact private scan repairs. Sol/medium is
a separate final review/GO runner, never a temporary switch inside the
interactive orchestrator session. Claude is not selected in this environment.
The `design-scout` and fresh-review profiles expose only
`read`, `grep`, `find`, and `ls`; it has no shell or mutation tools. A bridge
policy also blocks shell/write/edit calls defensively. Runtime experiments must
be requested from the orchestrator and run in an approved disposable VM, never
against the host system or user manager.

## User workflow

- The widget is always visible after `/reload`. Its evenly distributed,
  colour-coded segments are **User**, **Active** (with an animated worker
  spinner), **Review Queue**, and **Orchestrator** (`idle`, `reviewing`,
  `dispatching`, `scanning`, `monitoring`, or `waiting for user`). It has no redundant
  `Orch:` prefix.
- `/orch` opens a live keyboard-driven English overlay dashboard. It starts in
  **My Actions**; use `2` for **Active**, `3` for **All**, and `4` for **Usage**.
  Usage aggregates provider-reported input/output/cache tokens and nominal USD
  cost by phase and agent/model route, with a clearly labelled configurable EUR
  conversion. Finished (`done`) tasks are stably sorted below non-finished work
  in the All tab.
- Press `o` on a task to open/reuse a dedicated clean tmux viewer window. It
  subscribes to bounded daemon activity and usage and never displays RPC JSON.
  Future runner panes also render concise transcript/tool/usage lines instead
  of raw JSON, but remain an internal transport detail. The interactive orchestrator footer is replaced by
  the single centred identity `Orchestrator agent · Sol / medium`; detailed
  usage lives in `/orch` rather than cluttering the client.
- New `priority:P1` + `status:ready` tickets appear as admissions and never run
  without maintainer approval.
- `/backlog <request>` and `orch_backlog_draft` queue a read-only Terra/medium
  splitter. Its validated dependency DAG remains local until `/orch` shows the
  exact batch and the maintainer chooses Yes. Approved issues are created in
  topological order with real issue-number dependency links; partial batches
  never retry automatically.
- Red scan results return privately to the same subagent; a fully green
  preflight creates an isolated bounded fresh-review agent.
- Only policy/product questions and PR approval are placed in the maintainer
  inbox.
- Closing Pi does not stop the daemon or running agents; pending review and
  decisions remain durable.

## Durable states

Warm continuation is reserved for a still-live interactive turn. Settlement is
a durable boundary and therefore routes to a fresh handoff; this avoids paying
the full accumulated context cost on every review iteration.

Typical flow:

```text
pending admission → queued → dispatching → running → scanning
                                                   ├─ red → queued (private repair)
                                                   └─ green → preparing_review
                                                               → fresh review runner
                                                               ├─ approve → done
                                                               ├─ changes_requested → queued (private repair)
                                                               └─ blocked → waiting_decision
```

The daemon defaults to four concurrent isolated workers and enforces
same-worktree/branch exclusion. Expensive automatic scan plans are serialized by
default. Parallel shards never write the integration worktree; only the fresh
Luna/low integration phase applies their green patches.
Before dispatch it refreshes GitHub issue state and verifies worktree branch and
HEAD. A runner disappearing without settlement becomes `failed` after the
configured grace period.

## Current migrated work

Settled green implementation and verification work migrates to a fresh bounded
review runner rather than being injected into the interactive frontend. The
disposable smoke-test task remains `done`.

## Verification

Passing tests:

- `test/queue.test.mjs` — concurrency, worktree/branch exclusion, dependencies;
- `test/runner.test.mjs` — strict RPC relay, orchestrator-input event,
  exactly-once settlement persistence;
- `test/preflight.test.mjs` — quick-red short circuit and full green routing;
- `test/daemon-review.test.mjs` — legacy migration coverage, private red return,
  and bounded exceptional-review compatibility;
- `test/fresh-review.test.mjs` — green-to-isolated-review routing, owner-only
  change bundle, typed approve, and private changes-requested return;
- `test/daemon-crash.test.mjs` — missing runner reconciliation to `failed`;
- frontend RPC load — `/orch` command loads and the extension connects to the
  daemon without extension errors.

Still intentionally absent: automatic push, PR creation, merge, tag, publish,
or release. Those require a maintainer-approved decision and a separate typed
external-action implementation.
