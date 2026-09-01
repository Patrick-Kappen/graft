# Graft agent-orchestrator playbook

This is the operating guide for coordinating Pi subagents on Graft work. It is
an operations aid, not product documentation and not authority to change the
repository or GitHub.

## Operating model

- The orchestrator owns scope, sequencing, review, and external actions.
- Each subagent has one dedicated Git worktree and one tmux window in
  `graft-v04-pi`.
- The subagent owns investigation or implementation only within its prompt.
  It does not push, open or merge pull requests, tag, publish, alter issue
  priority, or make release decisions unless the maintainer explicitly asks.
- Systemd remains the scheduler and Podman remains the runtime authority.
  A future Graft worker may request typed lifecycle operations and report
  bounded observations, but is not an additional workload supervisor.

## Sources of truth

1. The current ticket and its acceptance criteria.
2. The current `origin/main` SHA and the exact worktree baseline.
3. `.plandocs/agent/handoff.md` for this release's authority, release gate, and
   currently authorized task.
4. The daemon's durable state at `~/.local/state/pi-orch/state.json`; runner
   settlements are ingested from `$XDG_RUNTIME_DIR/pi-orch/events.jsonl` and
   exposed through the daemon control socket.

Terminal text, a green command exit, and an agent's claimed completion are not
sufficient evidence by themselves. Inspect the reported artifacts and verify
observable effects.

## Before dispatch

1. Read the handoff and relevant ticket(s); resolve dependencies and avoid
   duplicating an existing design or child ticket.
2. Record the exact baseline SHA. Check `git status --short` in the target
   worktree before work begins.
3. Create or reuse one named worktree and one matching tmux window. Do not let
   two agents modify the same worktree or branch.
4. Give one bounded task. The prompt must state scope, forbidden actions,
   exact files or evidence to inspect, required tests, stop conditions, and the
   final state line:

   ```text
   STATE: done|blocked|failed — <one-sentence summary>
   ```

5. For a design/diagnostic task, explicitly prohibit tracked-file edits,
   commits, pushes, PRs, tags, releases, assertion weakening, and unapproved
   protocol changes.

## While agents run

- Watch the daemon event/subscription stream; do not infer completion solely
  from a quiet tmux pane.
- Use tmux capture only to inspect progress or recover the final report.
- Keep a task ledger: ticket, agent/session, worktree, baseline SHA, authorized
  scope, command/artifact evidence, outcome, and next gate.
- If scope expands, evidence conflicts, a test is flaky, or an agent needs a
  product decision, stop and ask the maintainer. Do not silently turn a
  diagnostic into implementation.

## Model, thinking, and review routing

Every model-backed task persists both a provider/model id and an explicit Pi
thinking level. Supported levels are `off`, `minimal`, `low`, `medium`, `high`,
`xhigh`, and `max`; Pi clamps a requested level to the selected model's actual
capabilities.

- deterministic scheduling and scans use no model;
- design/triage defaults to `openai-codex/gpt-5.6-terra:medium`;
- bounded routine implementation shards default to
  `opencode-go/deepseek-v4-flash:low` and may run concurrently in distinct
  worktrees;
- after all shards are green, a fresh
  `openai-codex/gpt-5.6-luna:low` integration agent applies their bounded
  patches in one clean integration worktree;
- high-risk protocol, security, or concurrency implementation uses
  `openai-codex/gpt-5.6-luna:high`;
- private repair of an exact red scan uses `low`;
- independent local verification uses
  `opencode-go/deepseek-v4-flash:low`;
- final semantic review and release-GO use a new isolated
  `openai-codex/gpt-5.6-sol:medium` subagent. The interactive orchestrator never
  performs this code review in its accumulated conversation context. Claude is
  not usable in this environment and must not be selected.

The default worker concurrency is four, while deterministic full-scan execution
is serialized by default so parallel coding does not launch several expensive
Nix/VM scan suites simultaneously. Routine reviews may remain queued until the
orchestrator is idle or their bounded defer window expires. P1 release blockers, security/protocol changes,
maintainer questions, and reviews that unblock an exclusive worktree are
immediate. Deferral never bypasses review and never advances a dependency.

## Completion and review

1. A settlement from an implementation or verification agent enters automatic
   preflight, not orchestrator review. Quick format/lint/integrity scans run
   first; expensive required local CI scans run only when the quick phase is
   green.
2. Red results are written to owner-only logs and returned privately to the
   same logical subagent through a fresh bounded repair handoff. Raw lint output
   is not inserted into the maintainer UI or orchestrator-agent context.
3. A green preflight creates a fresh, sessionless Sol/medium review task. The
   daemon generates an owner-only bounded bundle containing acceptance scope,
   worktree status, green scan names, and the baseline diff (including untracked
   additions). Implementation narrative and all prior Pi/orchestrator sessions
   are excluded. The reviewer has only read/grep/find/ls and emits a typed final
   verdict: approve, changes requested, or blocked.
4. Approve advances the parent dependency automatically. Changes requested are
   returned privately to the implementation agent through a fresh bounded
   repair handoff. Blocked or repeatedly malformed verdicts become maintainer
   decisions; they are not reviewed in the interactive orchestrator context.
5. Three consecutive red repair attempts surface one exceptional review item so
   infrastructure failures cannot consume unbounded agent turns.
6. Ask for explicit approval before GitHub mutations, branch integration, or
   release action.

## Backlog drafting and splitting

Both the maintainer and orchestrator may use `/backlog <request>` or the typed
`orch_backlog_draft` tool. This authorizes only a local draft. A fresh read-only
Terra/medium splitter produces at most ten small implementation-ready tickets as
a validated dependency DAG. It prefers independent shards, followed by an
explicit Luna/low integration ticket and later verification where needed.

The daemon validates ids, lengths, labels, dependencies, and acyclicity, then
topologically orders the batch. `/orch` presents the exact titles, bodies,
labels, and dependency order with **Yes / No / Explain**. Only **Yes** authorizes
creation of that bounded GitHub batch. Issues are created in dependency order;
real earlier issue numbers are inserted into later dependency sections. Every
issue receives `status:backlog`. A partial or interrupted batch is never retried
automatically, preventing silent duplicates; it becomes a maintainer inspection
decision. Batch approval does not approve implementation, push, PR, merge, tag,
or release.

## Standard gates and ordered pipelines

`diagnostic/design → maintainer pipeline admission → parallel isolated shards →
per-shard green scans → Luna/low integration → integrated green scans → fresh
Sol/medium review → local focused verification → exact-SHA central CI
approval/action → independent GO → release approval/action`

Admission applies once to the complete bounded ticket pipeline. Internal phases
are durable tasks linked by `dependsOn` and advance automatically only after the
predecessor result passes automatic preflight and fresh isolated semantic review.
A follow-up remains inside its phase; a blocked phase durably blocks its dependants. The maintainer is asked
again only for a genuine product decision or external mutation such as central
workflow dispatch, PR/push, merge, tag, or release. This prevents completed
internal work from stalling merely because a GitHub status label was not
mutated.

For release-blocking runtime defects, local retries never replace the required
central CI evidence.

## Current #368 routing

- #364 is the P1 design/POC ticket for the rootless Quadlet notify-attribution
  race.
- #368 is the P1 release-blocking ordered delivery pipeline: accepted #364
  design → implementation → independent local verification → approved central
  `runtime_tests=true` evidence → independent GO → separately approved release.
- Do not create a second workload supervisor as a workaround. Preserve the
  established systemd/Podman authority boundary.
- The worker/control-plane work is separate; it observes and requests typed
  operations but does not solve the service-start protocol.

## Proposed orchestration-extension POC

A small **project-local Pi extension** can remove the manual tmux/session
plumbing without changing the review and authorization gates above. The
feasibility results are recorded in [the technical spike](orchestration-extension-spike.md).

### Shape

- Register an `/orch-start` command and an `orchestrate_agent` tool. Accept a
  strict schema only: task id, agent/session id, trusted working directory,
  tmux session/window name, model/thinking selection, and prompt. Do not accept
  arbitrary shell or tmux command fragments.
- Idempotently create or reuse the named tmux session and window. Launch a
  small Node runner in that pane; the runner starts `pi --mode rpc` with the
  declared arguments.
- The runner owns the child Pi process's stdin/stdout and parses RPC JSONL
  strictly. Its pane shows human-readable progress on stderr/a log, so RPC
  stdout is never mixed with terminal output.
- The daemon owns one control socket plus one runner socket per active agent
  under `$XDG_RUNTIME_DIR/pi-orch/`. The Pi extension subscribes to the daemon;
  it never scrapes tmux panes. Runners forward RPC events and accept typed
  prompts through their sockets.
- Stream `message_update` and tool events as progress. Treat only
  `agent_settled` as completion; `agent_end` can still be followed by retry,
  compaction, or queued work. Capture the final assistant message separately.
- On settlement, append one structured event to the runtime event log. For
  implementation and verification profiles, the daemon first runs the trusted
  scan plan. Red returns privately to the same subagent; green creates a new
  read-only Sol/medium review runner with an owner-only bounded change bundle.
  No review prompt is injected into the interactive orchestrator Pi session.
  Events contain task/session ids, baseline SHA, final state, bounded artifact
  paths, and timestamps; they contain no secret values.

### POC acceptance criteria

1. Starting the same task twice reuses the intended tmux session/window and
   never creates a duplicate runner.
2. A prompt reaches the RPC child and live events reach the orchestrator.
3. `agent_settled` produces exactly one completion event and exactly one
   automatic route. Red produces a private repair handoff; only green produces
   an orchestrator review message.
4. Steer and abort are correlated to the right agent; a stale socket or dead
   pane is reported as `failed`, never as `done`.
5. No tokens, prompts containing secrets, or raw RPC stdout are written to the
   shared event log. The runner and socket are cleaned up on explicit stop;
   session shutdown only disconnects the caller and does not silently kill an
   authorized task.

This is orchestration infrastructure, not a release-blocker workaround; it
must not be given P1 ahead of #364/#368.

### Concurrent subagents with serialized per-agent work

Multiple subagents may run concurrently. The scheduler has a configured global
concurrency cap, while enforcing **at most one active turn per agent, queue
item, worktree, and branch**. A task is not a permanently running tmux pane; it
is a stateful queue item that may have several short, resumable agent turns.

```text
queued → running → settled → scanning
                              ├─ red → fresh private repair → queued
                              └─ green → orchestration-review
                                          ├─ resume-agent → queued
                                          ├─ maintainer-decision → waiting-approval
                                          ├─ ready-to-pr → waiting-pr-approval
                                          ├─ completed
                                          └─ failed | blocked
```

- A queue item includes a stable id, priority, worktree/branch, baseline SHA,
  agent configuration, prompt revision, Pi session file, evidence paths, and
  an append-only transition history. The queue manager uses an exclusive lock
  plus atomic state replacement; it must recover an interrupted `running` item
  as `failed` or `blocked`, never silently drop it.
- `agent_settled` releases the model turn and starts a daemon-owned scan phase;
  the worktree and global execution slot remain reserved until scanning ends.
  The daemon consumes settlement events in a serialized control queue, so scan,
  completion, and Git/worktree decisions cannot race. Red scans append a typed
  private follow-up automatically. If semantic review needs more work, it
  appends a typed follow-up and requeues the same item with a fresh bounded
  handoff while retaining the prior session as evidence; it does not start a second turn for that same
  item, agent, worktree, or branch.
- Before releasing the slot, the runner persists the final event and log/artifact
  references, acknowledges them to the extension, closes the Unix socket, and
  kills its tmux window. A later resume creates a fresh window and reconnects to
  the saved Pi session. Thus no idle agent panes accumulate and evidence is not
  lost when a window closes.
- `waiting-approval` and `waiting-pr-approval` consume no execution slot, so
  the scheduler may start other queued items up to the concurrency cap. A PR is never created, pushed,
  merged, tagged, or released merely because an agent says it is ready: the
  orchestrator asks the maintainer first, then performs only the approved
  action and records its result.
- Default scheduling is FIFO within priority among eligible items. A returned
  agent follow-up is a new queued turn, not an implicit monopoly; an explicit
  maintainer override may move it to the front when it blocks the active ticket.
- Shared integration actions (rebase, merge, release, or changes to a shared
  branch) are serialized separately even while independent diagnostic or
  implementation agents run in parallel.

Additional POC tests: prove the configured concurrency cap, rejection of a
second active turn for the same worktree/branch, serialized completion handling,
recovery after runner/tmux death, exactly-once completion persistence before
window removal, successful session resume in a fresh window, and a queued item
continuing while another waits for maintainer/PR approval.

### Maintainer decision inbox and per-agent mailbox

The orchestrator must turn an agent's request for a decision into a durable
maintainer-inbox item, rather than keeping the agent/pane open waiting for an
answer.

- A decision record has an id, task/agent/session ids, title, bounded evidence
  and links, the exact decision requested, allowed actions, and a transition
  history. It is persisted in the orchestrator state store and surfaced in the
  current Pi session as a non-model-visible custom entry.
- In the Pi TUI, the compact orchestrator status bar plus `/orch` opens a
  selectable **My Actions** inbox. Selecting an item shows its evidence and offers **Yes**,
  **No**, or **Explain**. `Explain` opens an editor and stores the maintainer's
  text verbatim as the decision response. Keyboard selection is required; mouse
  support, if added, is only a convenience.
- The decision command can be used while the orchestrator Pi is otherwise idle
  or while other agents run. It changes only the selected decision record;
  dispatch remains serialized.
- Each agent has a durable FIFO mailbox of typed follow-up prompts. Resolving a
  decision appends one message to that agent's mailbox. Several decisions or
  maintainer messages may be queued for the same agent.
- The dispatcher sends a mailbox message only after the target RPC agent reports
  `isStreaming: false` (idle). If its runner/window was already closed after the
  prior settlement, the dispatcher recreates the window, resumes the saved Pi
  session, confirms idle state, and then sends the first FIFO message. It never
  injects a decision as a mid-turn steer or silently drops/reorders mailbox
  messages.
- Once a message is accepted, the agent runs until its next `agent_settled`;
  the window is again closed after evidence persistence. The next mailbox item
  waits for a later dispatch cycle, preserving both per-agent ordering and the
  global concurrency/worktree limits.

POC acceptance additionally requires restart persistence of pending decisions,
exactly-once decision-to-mailbox delivery, a decision resolved from an idle TUI,
and two ordered follow-ups delivered to one resumed agent session.

### Session-recovery dashboard and cache-aware resumes

The durable queue state is also the source of truth when the orchestrator Pi
starts a **new** session. On `session_start`, the extension must reconstruct a
compact dashboard that identifies: active ticket/task and worktree, running
agents, next eligible queued items, pending decisions, blocked/failed items,
and the latest settled result. It must not infer this from a tmux pane or a
previous Pi conversation. A dashboard custom entry is non-model-visible; the
same bounded state is available through `/orch-status`.

Prompt caching is an optimisation, not persistent agent state. Provider TTLs
are not uniform: Anthropic's default prompt-cache TTL is 5 minutes; OpenAI
retention varies by API/model. Pi can request longer provider caching with
`PI_CACHE_RETENTION=long` where supported, but that is not a guarantee of a
cache hit. The runner therefore persists the provider/model, session file,
last-settled timestamp, and compact handoff metadata for every task.

For each queued mailbox message, the scheduler chooses explicitly:

1. **Warm resume:** same provider/model, saved session is valid, and the last
   settled turn is within a configurable conservative warm window (default
   4 minutes). Start a fresh RPC runner with that saved Pi session and deliver
   the next FIFO message.
2. **Cold handoff:** cache window is expired, provider/model changed, session
   is unavailable, or context is too large. Start a new named Pi session first,
   seed it with a bounded task/worktree/baseline/evidence/handoff summary, and
   then deliver the mailbox message. That summary includes a fresh read-only
   GitHub snapshot (issue/PR URL and state, labels, reviews/merge state,
   required CI status, and fetch timestamp).

The policy must record which route was selected and why. A cold handoff never
silently discards the old session; it retains its path and summary for review.

Before every dispatch, including a warm resume, the scheduler enforces a
**freshness-and-order gate**. It refreshes the GitHub snapshot if older than its
configured bound (default 5 minutes) and refuses dispatch if refresh fails or
leaves an ambiguous state. It then verifies the ticket's declared dependencies
and priority order, the expected branch/worktree, exact baseline SHA, and clean
or explicitly recorded local diff. A dependent ticket cannot run until its
predecessor has the required terminal state (for example merged, approved, or
an explicit maintainer override). Shared-branch integration remains serialized.

The gate writes the verified snapshot id/timestamp and dependency decision into
the queue history. A later GitHub change invalidates affected queued handoffs;
they return to `queued-stale` and must pass the gate again rather than running
on stale assumptions. POC tests must cover new-orchestrator-session dashboard
recovery, a warm resume inside the window, a cold new-session handoff after
expiry, expired GitHub data, and a blocked dependency.
