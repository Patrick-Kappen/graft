import assert from "node:assert/strict";
import { appendFile, chmod, mkdtemp, mkdir, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import os from "node:os";
import path from "node:path";
import { request } from "../client.mjs";

const extensionRoot = path.resolve(import.meta.dirname, "..");
const repo = path.resolve(extensionRoot, "../../../..");
const temp = await mkdtemp(path.join(os.tmpdir(), "graft-orch-daemon-"));
const runtime = path.join(temp, "runtime");
const stateRoot = path.join(temp, "state");
const stateDir = path.join(stateRoot, "pi-orch");
const runtimeDir = path.join(runtime, "pi-orch");
const fakeBin = path.join(temp, "bin");
await mkdir(stateDir, { recursive: true });
await mkdir(runtimeDir, { recursive: true });
await mkdir(fakeBin, { recursive: true });
await writeFile(path.join(fakeBin, "gh"), "#!/bin/sh\nprintf '%s\\n' '[]'\n", { mode: 0o700 });
await chmod(path.join(fakeBin, "gh"), 0o700);
const taskId = "migrated-review";
const followupId = "fresh-followup";
const retryId = "failed-safe-retry";
const verificationId = "pipeline-local-verification";
const scanRedId = "automatic-scan-red";
const scanGreenId = "automatic-scan-green";
const scanDeferredId = "automatic-scan-deferred";
const planId = "ticket-368-delivery";
await writeFile(
  path.join(stateDir, "state.json"),
  `${JSON.stringify({
    version: 1,
    tasks: {
      [taskId]: {
        id: taskId,
        ticket: 364,
        source: "admission",
        state: "done",
        createdAt: "2026-01-01T00:00:00.000Z",
        history: [],
        mailbox: [],
      },
      [retryId]: {
        id: retryId,
        ticket: 364,
        source: "admission",
        profile: "design",
        state: "failed",
        prompt: "Original retry scope",
        dispatchMessage: "Durable bounded retry handoff",
        worktree: "/tmp/failed-safe-retry",
        branch: "orch-failed-safe-retry",
        sessionFile: "/tmp/failed-unsafe-session.jsonl",
        createdAt: "2026-01-01T00:03:00.000Z",
        history: [],
        mailbox: [],
      },
      [verificationId]: {
        id: verificationId,
        ticket: 368,
        source: "pipeline",
        profile: "verification",
        phase: "local-verification",
        pipelinePlanId: planId,
        state: "awaiting_review",
        prompt: "Verify the implementation",
        worktree: "/tmp/pipeline-verification",
        branch: "orch-pipeline-verification",
        verifiedHead: "abcdef1234567890",
        result: { finalText: "verification passed" },
        createdAt: "2026-01-01T00:04:00.000Z",
        history: [],
        mailbox: [],
      },
      [scanRedId]: {
        id: scanRedId,
        ticket: 368,
        source: "pipeline",
        profile: "implementation",
        state: "running",
        prompt: "Fix the implementation",
        worktree: repo,
        branch: "scan-red",
        preflight: { kind: "test-red", maxAttempts: 3 },
        createdAt: "2026-01-01T00:05:00.000Z",
        history: [],
        mailbox: [],
      },
      [scanGreenId]: {
        id: scanGreenId,
        ticket: 368,
        source: "pipeline",
        profile: "implementation",
        state: "running",
        prompt: "Fix the implementation",
        worktree: repo,
        branch: "scan-green",
        preflight: { kind: "test-green", maxAttempts: 3 },
        createdAt: "2026-01-01T00:06:00.000Z",
        history: [],
        mailbox: [],
      },
      [scanDeferredId]: {
        id: scanDeferredId,
        ticket: 123,
        source: "manual",
        profile: "implementation",
        state: "running",
        prompt: "Routine implementation",
        worktree: repo,
        branch: "scan-deferred",
        preflight: { kind: "test-green", maxAttempts: 3 },
        reviewDelayMs: 60000,
        createdAt: "2026-01-01T00:06:30.000Z",
        history: [],
        mailbox: [],
      },
      [followupId]: {
        id: followupId,
        ticket: 364,
        source: "admission",
        profile: "design",
        state: "awaiting_review",
        prompt: "Original bounded design scope",
        worktree: "/tmp/fresh-followup",
        branch: "orch-fresh-followup",
        sessionFile: "/tmp/old-large-session.jsonl",
        result: { finalText: "prior evidence\nSTATE: done — first result" },
        createdAt: "2026-01-01T00:02:00.000Z",
        history: [],
        mailbox: [],
      },
    },
    admissions: { "999": { ticket: 999, title: "fixture", state: "pending", discoveredAt: "2026-01-01T00:00:00.000Z" } },
    decisions: {},
    plans: {
      [planId]: {
        id: planId,
        ticket: 368,
        state: "running",
        baseline: "baseline-sha",
        createdAt: "2026-01-01T00:00:00.000Z",
        phases: [{ id: "local-verification", taskId: verificationId }, { id: "central-ci", gate: "external-approval" }],
      },
    },
  })}\n`,
);
await writeFile(
  path.join(runtimeDir, "events.jsonl"),
  `${JSON.stringify({ eventId: "settled-1", type: "orchestrator_agent_settled", taskId, state: "done", finalText: "design result", timestamp: "2026-01-01T00:01:00.000Z" })}\n`,
);
const daemon = spawn(process.execPath, [path.join(extensionRoot, "daemon.mjs"), "--repo", repo], {
  env: {
    ...process.env,
    PATH: `${fakeBin}:${process.env.PATH}`,
    XDG_RUNTIME_DIR: runtime,
    XDG_STATE_HOME: stateRoot,
    GRAFT_ORCH_PAUSE_DISPATCH: "1",
    GRAFT_ORCH_ALLOW_TEST_PREFLIGHT: "1",
    GRAFT_ORCH_LEGACY_MAIN_REVIEW: "1",
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
let snapshot = await request(socket, { type: "snapshot" });
assert.equal(snapshot.tasks.find((task) => task.id === taskId).state, "awaiting_review");
assert.equal(snapshot.tasks.find((task) => task.id === taskId).result.finalText, "design result");
const migratedDesign = snapshot.tasks.find((task) => task.id === followupId);
assert.equal(migratedDesign.model, "openai-codex/gpt-5.6-terra");
assert.equal(migratedDesign.thinking, "medium");
assert.equal(migratedDesign.policy, "design-read-only");
assert.deepEqual(migratedDesign.tools, ["read", "grep", "find", "ls"]);
assert.ok(!migratedDesign.tools.includes("bash"));
const claimed = await request(socket, { type: "claim_review" });
assert.equal(claimed.id, taskId);
await request(socket, { type: "review_action", taskId, action: "complete", message: "accepted by orchestrator" });
snapshot = await request(socket, { type: "snapshot" });
assert.equal(snapshot.tasks.find((task) => task.id === taskId).state, "done");
assert.match(snapshot.tasks.find((task) => task.id === taskId).history.at(-1).detail, /accepted by orchestrator/);
await request(socket, { type: "review_action", taskId: followupId, action: "follow_up", message: "Validate only the remaining oracle." });
snapshot = await request(socket, { type: "snapshot" });
const fresh = snapshot.tasks.find((task) => task.id === followupId);
assert.equal(fresh.state, "queued");
assert.equal(fresh.resumeRoute, "fresh-handoff");
assert.notEqual(fresh.nextSessionFile, fresh.sessionFile);
assert.match(fresh.nextSessionFile, /followup-/);
assert.match(fresh.mailbox[0], /Fresh orchestrator handoff/);
assert.match(fresh.mailbox[0], /Validate only the remaining oracle/);
await request(socket, { type: "retry_failed", taskId: retryId, reason: "safe harness deployed" });
snapshot = await request(socket, { type: "snapshot" });
const retry = snapshot.tasks.find((task) => task.id === retryId);
assert.equal(retry.state, "queued");
assert.equal(retry.resumeRoute, "fresh-handoff-retry");
assert.match(retry.nextSessionFile, /-retry-/);
assert.equal(retry.mailbox[0], "Durable bounded retry handoff");
assert.match(retry.history.at(-1).detail, /safe harness deployed/);
await request(socket, { type: "review_action", taskId: verificationId, action: "complete", message: "local evidence accepted" });
snapshot = await request(socket, { type: "snapshot" });
const verification = snapshot.tasks.find((task) => task.id === verificationId);
assert.equal(verification.state, "done");
assert.equal(verification.policy, "verification-worktree");
assert.equal(verification.model, "opencode-go/deepseek-v4-flash");
assert.equal(verification.thinking, "low");
assert.deepEqual(verification.tools, ["read", "bash", "grep", "find", "ls"]);
const ciDecision = snapshot.decisions.find((decision) => decision.kind === "external_ci");
assert.ok(ciDecision);
assert.match(ciDecision.question, /runtime_tests=true/);
assert.equal(snapshot.plans.find((plan) => plan.id === planId).state, "waiting_external_approval");
await request(socket, { type: "resolve_decision", decisionId: ciDecision.id, answer: "yes" });
snapshot = await request(socket, { type: "snapshot" });
assert.equal(snapshot.tasks.find((task) => task.id === verificationId).state, "done");
assert.equal(snapshot.plans.find((plan) => plan.id === planId).state, "waiting_external_action");
await appendFile(
  path.join(runtimeDir, "events.jsonl"),
  [
    JSON.stringify({ eventId: "scan-red-settled", type: "orchestrator_agent_settled", taskId: scanRedId, state: "done", finalText: "agent claims done", timestamp: "2026-01-01T00:07:00.000Z" }),
    JSON.stringify({ eventId: "scan-green-settled", type: "orchestrator_agent_settled", taskId: scanGreenId, state: "done", finalText: "agent claims done", timestamp: "2026-01-01T00:07:01.000Z" }),
    JSON.stringify({ eventId: "scan-deferred-settled", type: "orchestrator_agent_settled", taskId: scanDeferredId, state: "done", finalText: "routine agent claims done", timestamp: "2026-01-01T00:07:02.000Z" }),
    "",
  ].join("\n"),
);
for (let attempt = 0; attempt < 100; attempt++) {
  snapshot = await request(socket, { type: "snapshot" });
  const red = snapshot.tasks.find((task) => task.id === scanRedId);
  const green = snapshot.tasks.find((task) => task.id === scanGreenId);
  const deferred = snapshot.tasks.find((task) => task.id === scanDeferredId);
  if (red?.state === "queued" && green?.state === "awaiting_review" && deferred?.state === "awaiting_review") break;
  if (attempt === 99) throw new Error(`automatic preflight did not settle: red=${red?.state} green=${green?.state} deferred=${deferred?.state}`);
  await new Promise((resolve) => setTimeout(resolve, 25));
}
const scanRed = snapshot.tasks.find((task) => task.id === scanRedId);
assert.equal(scanRed.preflightRun.state, "red");
assert.equal(scanRed.resumeRoute, "automatic-scan-repair");
assert.equal(scanRed.nextThinking, "low");
assert.match(scanRed.mailbox[0], /Model route: openai-codex\/gpt-5\.6-luna:low/);
assert.match(scanRed.mailbox[0], /not sent to the maintainer or orchestrator review/);
assert.doesNotMatch(scanRed.mailbox[0], /synthetic red scan/, "raw lint output must stay out of durable UI state");
assert.ok(!snapshot.tasks.find((task) => task.id === scanRedId).reviewClaimedAt);
const scanGreen = snapshot.tasks.find((task) => task.id === scanGreenId);
assert.equal(scanGreen.preflightRun.state, "green");
assert.equal(scanGreen.preflightRun.scans[0].code, 0);
const scanDeferred = snapshot.tasks.find((task) => task.id === scanDeferredId);
assert.equal(scanDeferred.model, "opencode-go/deepseek-v4-flash");
assert.equal(scanDeferred.thinking, "low");
assert.ok(Date.parse(scanDeferred.reviewNotBefore) > Date.now());
await request(socket, { type: "review_action", taskId: scanGreenId, action: "complete", message: "green fixture accepted" });
assert.equal(await request(socket, { type: "claim_review" }), null, "a routine review must remain deferred before reviewNotBefore");
await request(socket, { type: "park_admission", ticket: 999, explanation: "Niet voor deze release" });
snapshot = await request(socket, { type: "snapshot" });
assert.equal(snapshot.admissions[0].state, "parked");
assert.equal(snapshot.admissions[0].explanation, "Niet voor deze release");
const exit = daemon.exitCode === null ? new Promise((resolve) => daemon.once("exit", resolve)) : undefined;
daemon.kill("SIGTERM");
if (exit) await exit;
console.log("daemon migration and orchestrator-review test passed");
