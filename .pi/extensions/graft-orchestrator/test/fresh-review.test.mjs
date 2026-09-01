import assert from "node:assert/strict";
import { appendFile, chmod, mkdtemp, mkdir, readFile, stat, writeFile } from "node:fs/promises";
import { spawn, spawnSync } from "node:child_process";
import os from "node:os";
import path from "node:path";
import { request } from "../client.mjs";

const extensionRoot = path.resolve(import.meta.dirname, "..");
const temp = await mkdtemp(path.join(os.tmpdir(), "graft-orch-fresh-review-"));
const repo = path.join(temp, "repo");
const runtime = path.join(temp, "runtime");
const stateRoot = path.join(temp, "state");
const stateDir = path.join(stateRoot, "pi-orch");
const runtimeDir = path.join(runtime, "pi-orch");
const fakeBin = path.join(temp, "bin");
for (const directory of [repo, stateDir, runtimeDir, fakeBin]) await mkdir(directory, { recursive: true });
await writeFile(path.join(fakeBin, "gh"), "#!/bin/sh\nprintf '%s\\n' '[]'\n", { mode: 0o700 });
await chmod(path.join(fakeBin, "gh"), 0o700);

function git(...args) {
  const result = spawnSync("git", args, { cwd: repo, encoding: "utf8" });
  if (result.status !== 0) throw new Error(result.stderr || `git ${args.join(" ")} failed`);
  return result.stdout.trim();
}

git("init", "-b", "main");
git("config", "user.email", "test@example.invalid");
git("config", "user.name", "Fresh Review Test");
await writeFile(path.join(repo, "tracked.txt"), "before\n");
git("add", "tracked.txt");
git("commit", "-m", "baseline");
const baseline = git("rev-parse", "HEAD");
await writeFile(path.join(repo, "tracked.txt"), "after\n");
await writeFile(path.join(repo, "added.txt"), "new candidate file\n");

const approveId = "candidate-approve";
const changesId = "candidate-changes";
const makeTask = (id, createdAt) => ({
  id,
  ticket: 368,
  source: "pipeline",
  profile: "implementation",
  state: "running",
  prompt: `Acceptance scope for ${id}: preserve lifecycle semantics.`,
  worktree: repo,
  branch: "main",
  baseline,
  preflight: { kind: "test-green", maxAttempts: 3 },
  reviewDelayMs: 0,
  createdAt,
  history: [],
  mailbox: [],
});
await writeFile(
  path.join(stateDir, "state.json"),
  `${JSON.stringify({
    version: 1,
    tasks: {
      [approveId]: makeTask(approveId, "2026-01-01T00:00:00.000Z"),
      [changesId]: makeTask(changesId, "2026-01-01T00:00:01.000Z"),
    },
    admissions: {},
    decisions: {},
    plans: {},
  })}\n`,
);
await writeFile(path.join(runtimeDir, "events.jsonl"), "");

const daemon = spawn(process.execPath, [path.join(extensionRoot, "daemon.mjs"), "--repo", repo], {
  env: {
    ...process.env,
    PATH: `${fakeBin}:${process.env.PATH}`,
    XDG_RUNTIME_DIR: runtime,
    XDG_STATE_HOME: stateRoot,
    GRAFT_ORCH_PAUSE_DISPATCH: "1",
    GRAFT_ORCH_ALLOW_TEST_PREFLIGHT: "1",
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

await appendFile(
  path.join(runtimeDir, "events.jsonl"),
  [
    JSON.stringify({ eventId: "candidate-approve-settled", type: "orchestrator_agent_settled", taskId: approveId, state: "done", finalText: "PRIVATE IMPLEMENTER NARRATIVE APPROVE", timestamp: "2026-01-01T00:01:00.000Z", sessionFile: path.join(temp, "approve-session.jsonl"), usage: { turns: 2, input: 100, output: 40, reasoning: 10, cacheRead: 400, cacheWrite: 0, totalTokens: 540, costUsd: 0.01 } }),
    JSON.stringify({ eventId: "candidate-changes-settled", type: "orchestrator_agent_settled", taskId: changesId, state: "done", finalText: "PRIVATE IMPLEMENTER NARRATIVE CHANGES", timestamp: "2026-01-01T00:01:01.000Z" }),
    "",
  ].join("\n"),
);

let snapshot;
for (let attempt = 0; attempt < 200; attempt++) {
  snapshot = await request(socket, { type: "snapshot" });
  const approve = snapshot.tasks.find((task) => task.id === approveId);
  const changes = snapshot.tasks.find((task) => task.id === changesId);
  if (approve?.state === "waiting_review_agent" && changes?.state === "waiting_review_agent") break;
  if (attempt === 199) throw new Error(`fresh reviews were not queued: approve=${approve?.state} changes=${changes?.state}\n${stderr}`);
  await new Promise((resolve) => setTimeout(resolve, 25));
}

const approveParent = snapshot.tasks.find((task) => task.id === approveId);
const changesParent = snapshot.tasks.find((task) => task.id === changesId);
const approveReview = snapshot.tasks.find((task) => task.id === approveParent.reviewTaskId);
const changesReview = snapshot.tasks.find((task) => task.id === changesParent.reviewTaskId);
for (const review of [approveReview, changesReview]) {
  assert.equal(review.state, "queued");
  assert.equal(review.profile, "change-review");
  assert.equal(review.model, "openai-codex/gpt-5.6-sol");
  assert.equal(review.thinking, "medium");
  assert.equal(review.policy, "review-changes-read-only");
  assert.deepEqual(review.tools, ["read", "grep", "find", "ls"]);
  assert.doesNotMatch(review.prompt, /\.jsonl|Previous session|sessionFile/);
  assert.match(review.prompt, /generated bundle/i);
}
assert.equal(await request(socket, { type: "claim_review" }), null, "green candidates must not enter interactive orchestrator review");
const bundle = await readFile(approveReview.reviewBundlePath, "utf8");
assert.match(bundle, /Acceptance scope for candidate-approve/);
assert.match(bundle, /-before\n\+after/);
assert.match(bundle, /new candidate file/);
assert.doesNotMatch(bundle, /PRIVATE IMPLEMENTER NARRATIVE APPROVE/);
assert.equal((await stat(approveReview.reviewBundlePath)).mode & 0o777, 0o600);

await appendFile(
  path.join(runtimeDir, "events.jsonl"),
  [
    JSON.stringify({ eventId: "approve-review-settled", type: "orchestrator_agent_settled", taskId: approveReview.id, state: "done", finalText: "No concrete findings.\nREVIEW: approve — candidate changes satisfy the bounded scope", timestamp: "2026-01-01T00:02:00.000Z" }),
    JSON.stringify({ eventId: "changes-review-settled", type: "orchestrator_agent_settled", taskId: changesReview.id, state: "done", finalText: "High: tracked.txt loses the required terminator.\nREVIEW: changes_requested — restore the required terminator", timestamp: "2026-01-01T00:02:01.000Z" }),
    "",
  ].join("\n"),
);

for (let attempt = 0; attempt < 100; attempt++) {
  snapshot = await request(socket, { type: "snapshot" });
  const approve = snapshot.tasks.find((task) => task.id === approveId);
  const changes = snapshot.tasks.find((task) => task.id === changesId);
  if (approve?.state === "done" && changes?.state === "queued") break;
  if (attempt === 99) throw new Error(`fresh verdicts were not routed: approve=${approve?.state} changes=${changes?.state}\n${stderr}`);
  await new Promise((resolve) => setTimeout(resolve, 25));
}
const approved = snapshot.tasks.find((task) => task.id === approveId);
assert.match(approved.history.at(-1).detail, /fresh Sol\/medium review approved/);
assert.equal(approved.usage.totalTokens, 540);
assert.equal(snapshot.usageSummary.total.totalTokens, 540);
assert.equal(snapshot.usageSummary.phases.find((phase) => phase.id === "implementation").usage.cacheRead, 400);
assert.match(snapshot.usageSummary.currencyNote, /nominal model cost/);
const returned = snapshot.tasks.find((task) => task.id === changesId);
assert.equal(returned.resumeRoute, "fresh-review-repair");
assert.match(returned.nextSessionFile, /review-repair/);
assert.match(returned.mailbox[0], /restore the required terminator/);
assert.doesNotMatch(returned.mailbox[0], /PRIVATE IMPLEMENTER NARRATIVE APPROVE/);

const exit = daemon.exitCode === null ? new Promise((resolve) => daemon.once("exit", resolve)) : undefined;
daemon.kill("SIGTERM");
if (exit) await exit;
console.log("fresh bounded review routing test passed");
