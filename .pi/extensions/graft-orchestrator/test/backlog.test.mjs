import assert from "node:assert/strict";
import { appendFile, chmod, mkdtemp, mkdir, readFile, writeFile } from "node:fs/promises";
import { spawn, spawnSync } from "node:child_process";
import os from "node:os";
import path from "node:path";
import { request } from "../client.mjs";

const extensionRoot = path.resolve(import.meta.dirname, "..");
const temp = await mkdtemp(path.join(os.tmpdir(), "graft-orch-backlog-"));
const repo = path.join(temp, "repo");
const runtime = path.join(temp, "runtime");
const stateRoot = path.join(temp, "state");
const stateDir = path.join(stateRoot, "pi-orch");
const runtimeDir = path.join(runtime, "pi-orch");
const fakeBin = path.join(temp, "bin");
const ghLog = path.join(temp, "gh-create.jsonl");
const ghCounter = path.join(temp, "gh-counter");
for (const directory of [repo, stateDir, runtimeDir, fakeBin]) await mkdir(directory, { recursive: true });
const gh = `#!/usr/bin/env node
import fs from "node:fs";
const args = process.argv.slice(2);
if (args[0] === "issue" && args[1] === "list") { console.log("[]"); process.exit(0); }
if (args[0] === "issue" && args[1] === "create") {
  const get = (name) => args[args.indexOf(name) + 1];
  const labels = args.flatMap((value, index) => value === "--label" ? [args[index + 1]] : []);
  const current = fs.existsSync(process.env.GH_COUNTER) ? Number(fs.readFileSync(process.env.GH_COUNTER, "utf8")) : 500;
  const number = current + 1;
  fs.writeFileSync(process.env.GH_COUNTER, String(number));
  const bodyPath = get("--body-file");
  fs.appendFileSync(process.env.GH_LOG, JSON.stringify({ number, title: get("--title"), body: fs.readFileSync(bodyPath, "utf8"), labels }) + "\\n");
  console.log("https://github.com/Patrick-Kappen/graft/issues/" + number);
  process.exit(0);
}
console.error("unexpected gh args: " + JSON.stringify(args));
process.exit(2);
`;
await writeFile(path.join(fakeBin, "gh"), gh, { mode: 0o700 });
await chmod(path.join(fakeBin, "gh"), 0o700);

function git(...args) {
  const result = spawnSync("git", args, { cwd: repo, encoding: "utf8" });
  if (result.status !== 0) throw new Error(result.stderr);
  return result.stdout.trim();
}
git("init", "-b", "main");
git("config", "user.email", "test@example.invalid");
git("config", "user.name", "Backlog Test");
await writeFile(path.join(repo, "README.md"), "fixture\n");
git("add", "README.md");
git("commit", "-m", "fixture");
const baseline = git("rev-parse", "HEAD");
const splitterId = "backlog-split-fixture";
await writeFile(path.join(stateDir, "state.json"), `${JSON.stringify({
  version: 1,
  tasks: {
    [splitterId]: {
      id: splitterId,
      phase: "backlog-splitting",
      profile: "backlog-splitter",
      source: "backlog-draft",
      state: "running",
      prompt: "Split a broad feature",
      requestedBy: "maintainer-command",
      requestedMaxTickets: 4,
      worktree: repo,
      branch: "main",
      baseline,
      createdAt: "2026-01-01T00:00:00.000Z",
      history: [],
      mailbox: [],
    },
  },
  admissions: {}, decisions: {}, plans: {}, backlogDrafts: {},
})}\n`);
await writeFile(path.join(runtimeDir, "events.jsonl"), "");
const daemon = spawn(process.execPath, [path.join(extensionRoot, "daemon.mjs"), "--repo", repo], {
  env: {
    ...process.env,
    PATH: `${fakeBin}:${process.env.PATH}`,
    GH_LOG: ghLog,
    GH_COUNTER: ghCounter,
    XDG_RUNTIME_DIR: runtime,
    XDG_STATE_HOME: stateRoot,
    GRAFT_ORCH_PAUSE_DISPATCH: "1",
  },
  stdio: ["ignore", "ignore", "pipe"],
});
let stderr = "";
daemon.stderr.on("data", (chunk) => (stderr += chunk.toString("utf8")));
const socket = path.join(runtimeDir, "control.sock");
for (let attempt = 0; attempt < 100; attempt++) {
  try { await request(socket, { type: "ping" }, 250); break; }
  catch { await new Promise((resolve) => setTimeout(resolve, 25)); }
  if (attempt === 99) throw new Error(`daemon not ready: ${stderr}`);
}
const plan = {
  title: "Ordered fixture backlog",
  summary: "Two small tickets with an integration dependency.",
  tickets: [
    { id: "integrate", title: "Integrate the core", body: "## Scope\nIntegrate.\n\n## Acceptance criteria\nGreen.\n\n## Tests/evidence\nFull suite.\n\n## Non-goals\nRelease.", labels: ["status:backlog", "type:quality"], dependsOn: ["core"] },
    { id: "core", title: "Implement the core", body: "## Scope\nCore only.\n\n## Acceptance criteria\nWorks.\n\n## Tests/evidence\nUnit test.\n\n## Non-goals\nIntegration.", labels: ["type:feature"], dependsOn: [] },
  ],
};
await appendFile(path.join(runtimeDir, "events.jsonl"), `${JSON.stringify({ eventId: "backlog-settled", type: "orchestrator_agent_settled", taskId: splitterId, state: "done", finalText: `Draft ready.\nBACKLOG_PLAN_JSON\n${JSON.stringify(plan)}\nEND_BACKLOG_PLAN`, timestamp: "2026-01-01T00:01:00.000Z" })}\n`);
let snapshot;
for (let attempt = 0; attempt < 100; attempt++) {
  snapshot = await request(socket, { type: "snapshot" });
  if (snapshot.decisions.some((decision) => decision.kind === "backlog_create" && decision.state === "pending")) break;
  if (attempt === 99) throw new Error(`backlog approval was not created: ${stderr}`);
  await new Promise((resolve) => setTimeout(resolve, 25));
}
const draft = snapshot.backlogDrafts[0];
assert.equal(draft.state, "pending_approval");
assert.deepEqual(draft.tickets.map((ticket) => ticket.id), ["core", "integrate"], "dependencies must be topologically ordered");
assert.deepEqual(draft.tickets[0].labels, ["status:backlog", "type:feature"]);
await assert.rejects(readFile(ghLog, "utf8"), /ENOENT/, "drafting must not mutate GitHub");
const decision = snapshot.decisions.find((item) => item.kind === "backlog_create");
assert.match(decision.question, /does not approve implementation, push, PR, merge, or release/);
await request(socket, { type: "resolve_decision", decisionId: decision.id, answer: "yes" });
for (let attempt = 0; attempt < 100; attempt++) {
  snapshot = await request(socket, { type: "snapshot" });
  if (snapshot.backlogDrafts[0]?.state === "created") break;
  if (attempt === 99) throw new Error(`approved backlog was not created: ${snapshot.backlogDrafts[0]?.state} ${stderr}`);
  await new Promise((resolve) => setTimeout(resolve, 25));
}
const created = (await readFile(ghLog, "utf8")).trim().split("\n").map(JSON.parse);
assert.deepEqual(created.map((item) => item.title), ["Implement the core", "Integrate the core"]);
assert.ok(created[0].labels.includes("status:backlog"));
assert.match(created[0].body, /## Dependencies\n- None/);
assert.match(created[1].body, /Depends on #501/);
assert.match(created[1].body, /graft-backlog-draft:/);
assert.deepEqual(snapshot.backlogDrafts[0].createdIssues.map((item) => item.number), [501, 502]);

const exit = daemon.exitCode === null ? new Promise((resolve) => daemon.once("exit", resolve)) : undefined;
daemon.kill("SIGTERM");
if (exit) await exit;
console.log("ordered backlog draft and approval test passed");
