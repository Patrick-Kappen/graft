#!/usr/bin/env node
import { randomUUID } from "node:crypto";
import { spawn } from "node:child_process";
import { chmod, mkdir, open, readFile, rename, rm, stat, writeFile } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { StringDecoder } from "node:string_decoder";
import { fetchIssueSnapshot } from "./github.mjs";
import { scanPlan, runScanPlan } from "./preflight.mjs";
import { dependenciesSatisfied, nextEligible, transition, withStateLock } from "./queue.mjs";

const args = process.argv.slice(2);
const repoIndex = args.indexOf("--repo");
if (repoIndex < 0 || !args[repoIndex + 1]) throw new Error("expected --repo <path>");
const repo = path.resolve(args[repoIndex + 1]);
const runtimeRoot = process.env.GRAFT_ORCH_RUNTIME_ROOT ?? path.join(process.env.XDG_RUNTIME_DIR ?? "/tmp", "pi-orch");
const stateRoot = process.env.GRAFT_ORCH_STATE_ROOT ?? path.join(process.env.XDG_STATE_HOME ?? path.join(os.homedir(), ".local/state"), "pi-orch");
const statePath = path.join(stateRoot, "state.json");
const controlSocket = path.join(runtimeRoot, "control.sock");
const eventLog = path.join(runtimeRoot, "events.jsonl");
const tmuxSession = process.env.GRAFT_ORCH_TMUX_SESSION ?? "graft-v04-pi";
const concurrency = Math.max(1, Math.min(8, Number.parseInt(process.env.GRAFT_ORCH_MAX_CONCURRENCY ?? "4", 10) || 4));
const scanConcurrency = Math.max(1, Math.min(2, Number.parseInt(process.env.GRAFT_ORCH_MAX_SCAN_CONCURRENCY ?? "1", 10) || 1));
const runnerGraceMs = Math.max(1000, Number.parseInt(process.env.GRAFT_ORCH_RUNNER_GRACE_MS ?? "10000", 10) || 10000);
const pauseDispatch = process.env.GRAFT_ORCH_PAUSE_DISPATCH === "1";
const routineReviewDelayMs = Math.max(0, Math.min(60 * 60_000, Number.parseInt(process.env.GRAFT_ORCH_ROUTINE_REVIEW_DELAY_MS ?? "600000", 10) || 0));
const legacyMainReview = process.env.GRAFT_ORCH_LEGACY_MAIN_REVIEW === "1";
const extensionRoot = path.dirname(new URL(import.meta.url).pathname);
const subscribers = new Set();
const activity = new Map();
const scanProcesses = new Map();
const scanWaiters = [];
let activeScans = 0;
let eventOffset = 0;
let scheduling = false;
let stopping = false;

function emptyUsage() {
  return { turns: 0, input: 0, output: 0, reasoning: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, costUsd: 0 };
}

function mergeUsage(target, value) {
  if (!value || typeof value !== "object") return target;
  target.turns += Number(value.turns) || 0;
  for (const key of ["input", "output", "reasoning", "cacheRead", "cacheWrite", "totalTokens"]) target[key] += Number(value[key]) || 0;
  target.costUsd += Number(value.costUsd ?? value.cost?.total) || 0;
  return target;
}

async function readSessionUsage(sessionFile) {
  const total = emptyUsage();
  let content;
  try { content = await readFile(sessionFile, "utf8"); }
  catch (error) { if (error.code === "ENOENT") return total; throw error; }
  for (const line of content.split("\n")) {
    if (!line) continue;
    let record;
    try { record = JSON.parse(line); } catch { continue; }
    const usage = record.message?.usage ?? (["compaction", "branch_summary"].includes(record.type) ? record.usage : undefined);
    if (!usage) continue;
    mergeUsage(total, { ...usage, turns: 1 });
  }
  return total;
}

function taskUsageFromSessions(task) {
  const total = emptyUsage();
  for (const usage of Object.values(task.sessionUsage ?? {})) mergeUsage(total, usage);
  return total;
}

async function refreshTaskUsage(taskId) {
  const task = await withStateLock(statePath, (state) => {
    const current = state.tasks[taskId];
    return current ? { sessionFile: current.sessionFile, sessionHistory: [...(current.sessionHistory ?? [])] } : undefined;
  });
  if (!task) return;
  const updates = {};
  for (const sessionFile of [...new Set([...task.sessionHistory, task.sessionFile].filter(Boolean))]) updates[sessionFile] = await readSessionUsage(sessionFile);
  await withStateLock(statePath, (state) => {
    const current = state.tasks[taskId];
    if (!current) return;
    current.sessionUsage = { ...(current.sessionUsage ?? {}), ...updates };
    current.usage = taskUsageFromSessions(current);
  });
}

function summarizeUsage(tasks) {
  const usdToEur = Math.max(0, Number.parseFloat(process.env.GRAFT_ORCH_USD_TO_EUR ?? "0.86237") || 0);
  const total = emptyUsage();
  const phases = new Map();
  const agents = new Map();
  for (const task of tasks) {
    const usage = task.usage;
    if (!usage || !(usage.turns || usage.totalTokens || usage.costUsd)) continue;
    mergeUsage(total, usage);
    const phaseKey = task.phase ?? task.profile ?? "unspecified";
    const agentKey = `${task.agent ?? task.profile ?? "agent"} · ${task.model ?? "default"}:${task.thinkingLevel ?? task.thinking ?? "default"}`;
    if (!phases.has(phaseKey)) phases.set(phaseKey, { id: phaseKey, usage: emptyUsage(), agents: new Set(), tasks: 0 });
    const phase = phases.get(phaseKey);
    mergeUsage(phase.usage, usage);
    phase.agents.add(agentKey);
    phase.tasks += 1;
    if (!agents.has(agentKey)) agents.set(agentKey, { id: agentKey, usage: emptyUsage(), phases: new Set(), tasks: 0 });
    const agent = agents.get(agentKey);
    mergeUsage(agent.usage, usage);
    agent.phases.add(phaseKey);
    agent.tasks += 1;
  }
  const finish = (item) => ({
    ...item,
    usage: { ...item.usage, costEur: item.usage.costUsd * usdToEur },
    ...(item.agents ? { agents: [...item.agents] } : {}),
    ...(item.phases ? { phases: [...item.phases] } : {}),
  });
  return {
    currencyNote: `Pi-reported nominal model cost; EUR estimate uses USD×${usdToEur}`,
    usdToEur,
    total: { ...total, costEur: total.costUsd * usdToEur },
    phases: [...phases.values()].map(finish).sort((a, b) => b.usage.costUsd - a.usage.costUsd),
    agents: [...agents.values()].map(finish).sort((a, b) => b.usage.costUsd - a.usage.costUsd),
  };
}

async function acquireScanSlot() {
  if (activeScans < scanConcurrency) {
    activeScans += 1;
    return;
  }
  await new Promise((resolve) => scanWaiters.push(resolve));
}

function releaseScanSlot() {
  const waiter = scanWaiters.shift();
  if (waiter) waiter();
  else activeScans = Math.max(0, activeScans - 1);
}

function attachJsonl(stream, onRecord, onError) {
  const decoder = new StringDecoder("utf8");
  let buffer = "";
  stream.on("data", (chunk) => {
    buffer += decoder.write(chunk);
    while (true) {
      const newline = buffer.indexOf("\n");
      if (newline < 0) break;
      let line = buffer.slice(0, newline);
      buffer = buffer.slice(newline + 1);
      if (line.endsWith("\r")) line = line.slice(0, -1);
      if (!line) continue;
      try { onRecord(JSON.parse(line)); }
      catch (error) { onError(new Error(`invalid JSONL: ${error instanceof Error ? error.message : String(error)}`)); }
    }
  });
}

async function exec(command, argv, options = {}) {
  return await new Promise((resolve, reject) => {
    const child = spawn(command, argv, { cwd: options.cwd ?? repo, shell: false, stdio: ["ignore", "pipe", "pipe"] });
    let stdout = "";
    let stderr = "";
    const timer = options.timeout
      ? setTimeout(() => { child.kill("SIGTERM"); reject(new Error(`${command} timed out`)); }, options.timeout)
      : undefined;
    child.stdout.on("data", (chunk) => (stdout += chunk.toString("utf8")));
    child.stderr.on("data", (chunk) => (stderr += chunk.toString("utf8")));
    child.on("error", reject);
    child.on("exit", (code) => {
      if (timer) clearTimeout(timer);
      resolve({ code: code ?? 1, stdout, stderr });
    });
  });
}

function appendTail(current, chunk, maximum = 128 * 1024) {
  const next = `${current}${chunk.toString("utf8")}`;
  return next.length <= maximum ? next : next.slice(-maximum);
}

async function executeScan(task, attempt, scan) {
  const logDir = path.join(stateRoot, "preflight", task.id);
  const logPath = path.join(logDir, `${attempt}-${scan.id}.log`);
  await mkdir(logDir, { recursive: true, mode: 0o700 });
  return await new Promise((resolve) => {
    const child = spawn(scan.command, scan.args, { cwd: task.worktree, shell: false, stdio: ["ignore", "pipe", "pipe"] });
    scanProcesses.set(`${task.id}:${scan.id}`, child);
    let stdout = "";
    let stderr = "";
    let timedOut = false;
    let finished = false;
    const finish = async (code) => {
      if (finished) return;
      finished = true;
      clearTimeout(timer);
      scanProcesses.delete(`${task.id}:${scan.id}`);
      await writeFile(logPath, [`command: ${scan.command} ${scan.args.map((value) => JSON.stringify(value)).join(" ")}`, `exit: ${code ?? 1}${timedOut ? " (timeout)" : ""}`, "", "--- stdout (bounded tail) ---", stdout, "", "--- stderr (bounded tail) ---", stderr, ""].join("\n"), { mode: 0o600 });
      resolve({ code: code ?? 1, stdout, stderr, timedOut, logPath });
    };
    const timer = setTimeout(() => {
      timedOut = true;
      child.kill("SIGTERM");
      setTimeout(() => child.kill("SIGKILL"), 5000).unref();
    }, scan.timeout);
    child.stdout.on("data", (chunk) => { stdout = appendTail(stdout, chunk); });
    child.stderr.on("data", (chunk) => { stderr = appendTail(stderr, chunk); });
    child.once("error", (error) => { stderr = appendTail(stderr, error.message); void finish(1); });
    child.once("exit", (code) => void finish(timedOut ? 124 : code));
  });
}

async function exists(filePath) {
  try { await stat(filePath); return true; }
  catch { return false; }
}

function writeRecord(socket, value) {
  if (!socket.destroyed) socket.write(`${JSON.stringify(value)}\n`);
}

async function loadSnapshot() {
  const raw = await readFile(statePath, "utf8").catch((error) => error.code === "ENOENT" ? '{"version":1,"tasks":{},"decisions":{},"admissions":{}}' : Promise.reject(error));
  const state = JSON.parse(raw);
  const tasks = Object.values(state.tasks ?? {}).sort((a, b) => a.createdAt.localeCompare(b.createdAt));
  return {
    type: "snapshot",
    timestamp: new Date().toISOString(),
    concurrency,
    tasks,
    usageSummary: summarizeUsage(tasks),
    admissions: Object.values(state.admissions ?? {}).sort((a, b) => a.ticket - b.ticket),
    decisions: Object.values(state.decisions ?? {}).sort((a, b) => a.createdAt.localeCompare(b.createdAt)),
    plans: Object.values(state.plans ?? {}).sort((a, b) => a.createdAt.localeCompare(b.createdAt)),
    backlogDrafts: Object.values(state.backlogDrafts ?? {}).sort((a, b) => a.createdAt.localeCompare(b.createdAt)),
    activities: Object.fromEntries(activity),
  };
}

async function broadcast(value) {
  for (const socket of subscribers) writeRecord(socket, value);
  if (value.type !== "activity") {
    const snapshot = await loadSnapshot();
    for (const socket of subscribers) writeRecord(socket, snapshot);
  }
}

async function latestSettlementEvents() {
  try {
    const lines = (await readFile(eventLog, "utf8")).split("\n").filter(Boolean);
    const map = new Map();
    for (const line of lines) {
      const event = JSON.parse(line);
      if (event.type === "orchestrator_agent_settled" && event.taskId) map.set(event.taskId, event);
    }
    return map;
  } catch (error) {
    if (error.code === "ENOENT") return new Map();
    throw error;
  }
}

async function migrate() {
  const settlements = await latestSettlementEvents();
  await withStateLock(statePath, async (state) => {
    state.admissions ??= {};
    state.decisions ??= {};
    state.plans ??= {};
    state.backlogDrafts ??= {};
    state.processedEvents ??= [];
    state.reviewQueue ??= [];
    for (const draft of Object.values(state.backlogDrafts)) {
      if (draft.state !== "creating") continue;
      draft.state = "partial_failed";
      draft.error = "daemon restarted during approved GitHub issue creation; automatic retry is disabled to avoid duplicates";
      draft.updatedAt = new Date().toISOString();
      if (!Object.values(state.decisions).some((decision) => decision.draftId === draft.id && decision.state === "pending")) {
        const id = randomUUID();
        state.decisions[id] = {
          id,
          taskId: draft.taskId,
          draftId: draft.id,
          kind: "decision",
          title: "Inspect interrupted backlog creation",
          question: `${draft.error}. ${draft.createdIssues?.length ?? 0}/${draft.tickets?.length ?? 0} creations were durably recorded. Inspect GitHub before drafting any remainder.`,
          state: "pending",
          createdAt: new Date().toISOString(),
        };
      }
    }
    for (const task of Object.values(state.tasks)) {
      task.mailbox ??= [];
      task.history ??= [];
      if (task.source === "pipeline") task.risk ??= "high";
      task.reviewDelayMs ??= task.risk === "high" || task.source === "admission" ? 0 : routineReviewDelayMs;
      if (task.profile === "design") {
        task.agent ??= "design-scout";
        task.model ??= "openai-codex/gpt-5.6-terra";
        task.thinking ??= "medium";
        task.policy = "design-read-only";
        task.tools = ["read", "grep", "find", "ls"];
      } else if (task.profile === "verification") {
        task.agent ??= "verification-scout";
        if (task.state === "queued") {
          task.model = "opencode-go/deepseek-v4-flash";
          task.thinking = "low";
        }
        task.model ??= "opencode-go/deepseek-v4-flash";
        task.thinking ??= "low";
        task.policy = "verification-worktree";
        task.tools = ["read", "bash", "grep", "find", "ls"];
      } else if (task.profile === "implementation") {
        task.agent ??= "implementation-worker";
        task.model ??= task.risk === "high" ? "openai-codex/gpt-5.6-luna" : "opencode-go/deepseek-v4-flash";
        task.thinking ??= task.risk === "high" ? "high" : "low";
        task.policy = "implementation-worktree";
        task.tools = ["read", "bash", "edit", "write", "grep", "find", "ls"];
      } else if (task.profile === "integration") {
        task.agent = "integration-worker";
        task.model = "openai-codex/gpt-5.6-luna";
        task.thinking = "low";
        task.policy = "implementation-worktree";
        task.tools = ["read", "bash", "edit", "write", "grep", "find", "ls"];
      } else if (task.profile === "backlog-splitter") {
        task.agent = "backlog-splitter";
        task.model = "openai-codex/gpt-5.6-terra";
        task.thinking = "medium";
        task.policy = "design-read-only";
        task.tools = ["read", "grep", "find", "ls"];
        task.preflight = undefined;
      } else if (task.profile === "change-review") {
        task.agent = "fresh-change-reviewer";
        task.model = "openai-codex/gpt-5.6-sol";
        task.thinking = "medium";
        task.policy = "review-changes-read-only";
        task.tools = ["read", "grep", "find", "ls"];
        task.preflight = undefined;
      }
      if (["implementation", "integration", "verification"].includes(task.profile)) {
        task.preflight ??= { kind: "required-local-ci", maxAttempts: 3 };
      }
      if (task.model) task.modelKey = task.model;
      task.sessionUsage ??= {};
      for (const sessionFile of [...new Set([...(task.sessionHistory ?? []), task.sessionFile].filter(Boolean))]) {
        task.sessionUsage[sessionFile] = await readSessionUsage(sessionFile);
      }
      task.usage = taskUsageFromSessions(task);
      const settlement = settlements.get(task.id);
      if (settlement && !task.result) task.result = settlement;
      if (task.source === "admission" && task.state === "done" && !task.reviewedAt) {
        task.state = "awaiting_review";
        task.history.push({ state: "awaiting_review", detail: "migrated settled agent result for orchestrator review", timestamp: new Date().toISOString() });
        if (!state.reviewQueue.includes(task.id)) state.reviewQueue.push(task.id);
      }
    }
  });
  eventOffset = await stat(eventLog).then((value) => value.size).catch((error) => error.code === "ENOENT" ? 0 : Promise.reject(error));
}

async function syncAdmissions() {
  const result = await exec("gh", ["issue", "list", "-R", "Patrick-Kappen/graft", "--state", "open", "--label", "priority:P1", "--label", "status:ready", "--json", "number,title,body,labels,url"], { timeout: 15000 });
  if (result.code !== 0) throw new Error(result.stderr || "gh issue list failed");
  const issues = JSON.parse(result.stdout);
  await withStateLock(statePath, (state) => {
    state.admissions ??= {};
    for (const issue of issues) {
      const key = String(issue.number);
      if (state.admissions[key] || Object.values(state.tasks).some((task) => task.ticket === issue.number && task.source === "admission")) continue;
      const design = /^design\b|\bdesign\(/i.test(issue.title);
      state.admissions[key] = {
        ticket: issue.number,
        title: issue.title,
        body: issue.body ?? "",
        url: issue.url,
        profile: design ? "design" : "implementation",
        agent: design ? "design-scout" : "implementation-worker",
        model: design ? "openai-codex/gpt-5.6-terra" : "opencode-go/deepseek-v4-flash",
        thinking: design ? "medium" : "low",
        tools: design ? ["read", "grep", "find", "ls"] : ["read", "bash", "edit", "write", "grep", "find", "ls"],
        policy: design ? "design-read-only" : "implementation-worktree",
        scope: design ? "read-only design/diagnostic" : "ticket-scoped implementation; no external mutations",
        state: "pending",
        discoveredAt: new Date().toISOString(),
      };
    }
  });
  await broadcast({ type: "admissions_refreshed", timestamp: new Date().toISOString() });
}

async function approveAdmission(ticket) {
  const admission = await withStateLock(statePath, (state) => {
    const item = state.admissions?.[String(ticket)];
    if (!item || item.state !== "pending") throw new Error(`ticket #${ticket} is not pending admission`);
    return { ...item };
  });
  const baselineResult = await exec("git", ["rev-parse", "origin/main"], { timeout: 5000 });
  if (baselineResult.code !== 0 || !baselineResult.stdout.trim()) throw new Error("cannot resolve origin/main baseline");
  const baseline = baselineResult.stdout.trim();
  const stamp = Date.now();
  const taskId = `ticket-${ticket}-${stamp}`;
  const branch = `orch-${ticket}-${stamp}`;
  const worktree = path.join("/tmp", `graft-orch-${ticket}-${stamp}`);
  const created = await exec("git", ["worktree", "add", "-b", branch, worktree, baseline], { timeout: 30000 });
  if (created.code !== 0) throw new Error(created.stderr || "worktree creation failed");
  await withStateLock(statePath, (state) => {
    const current = state.admissions[String(ticket)];
    if (!current || current.state !== "pending") throw new Error("admission changed during worktree creation");
    const now = new Date().toISOString();
    state.tasks[taskId] = {
      id: taskId,
      ticket,
      dependsOn: [],
      worktree,
      branch,
      baseline,
      prompt: [
        `Approved orchestrator ticket #${ticket}: ${admission.title}`,
        admission.body,
        admission.profile === "design"
          ? "Design/diagnostic only. Do not modify tracked files, commit, push, open a PR, tag, publish, or mutate GitHub."
          : "Implement only the ticket scope. Do not commit, push, open a PR, tag, publish, or mutate GitHub. Request orchestrator review when complete.",
        "End exactly: STATE: done|blocked|failed — <one-sentence summary>.",
      ].join("\n\n"),
      model: admission.model,
      thinking: admission.thinking ?? "medium",
      tools: admission.tools,
      profile: admission.profile,
      source: "admission",
      preflight: admission.profile === "implementation" ? { kind: "required-local-ci", maxAttempts: 3 } : undefined,
      reviewDelayMs: 0,
      state: "queued",
      createdAt: now,
      history: [{ state: "queued", detail: "maintainer admitted GitHub-ready ticket", timestamp: now }],
      mailbox: [],
    };
    current.state = "admitted";
    current.taskId = taskId;
  });
  await broadcast({ type: "admission_approved", ticket, taskId });
  void schedule();
  return taskId;
}

async function draftBacklog(request, maxTickets = 6, requestedBy = "orchestrator") {
  if (typeof request !== "string" || !request.trim()) throw new Error("backlog request must be non-empty");
  if (request.length > 12_000) throw new Error("backlog request exceeds 12000 characters");
  const boundedMaximum = Math.max(1, Math.min(10, Number.parseInt(String(maxTickets), 10) || 6));
  const branchResult = await exec("git", ["-C", repo, "branch", "--show-current"], { timeout: 5000 });
  const baselineResult = await exec("git", ["-C", repo, "rev-parse", "HEAD"], { timeout: 5000 });
  if (branchResult.code !== 0 || baselineResult.code !== 0) throw new Error("cannot resolve repository branch/baseline for backlog drafting");
  const stamp = Date.now();
  const taskId = `backlog-split-${stamp}`;
  await withStateLock(statePath, (state) => {
    const now = new Date().toISOString();
    state.tasks[taskId] = {
      id: taskId,
      phase: "backlog-splitting",
      dependsOn: [],
      worktree: repo,
      branch: branchResult.stdout.trim(),
      baseline: baselineResult.stdout.trim(),
      prompt: [
        "Create a small, implementation-ready GitHub backlog decomposition for the request below.",
        `Request:\n${request.trim()}`,
        `Produce at most ${boundedMaximum} tickets. Prefer independently implementable shards that can run concurrently in isolated worktrees. Put unavoidable integration and verification work after their dependencies. Keep each ticket small enough for one bounded agent turn.`,
        "Each ticket body must include Scope, Acceptance criteria, Tests/evidence, and Non-goals. Use only these labels: status:backlog, priority:P1, type:bug, type:chore, type:design, type:docs, type:feature, type:quality. Every ticket must include status:backlog and at most one type label. Do not use GitHub numbers; dependencies reference plan-local ids.",
        "Return a topologically valid plan. End with exactly these markers and valid JSON between them:\nBACKLOG_PLAN_JSON\n{\"title\":\"...\",\"summary\":\"...\",\"tickets\":[{\"id\":\"short-id\",\"title\":\"...\",\"body\":\"...\",\"labels\":[\"status:backlog\",\"type:feature\"],\"dependsOn\":[]}]}\nEND_BACKLOG_PLAN",
        "Do not modify files, run commands, create issues, or inspect prior Pi sessions.",
      ].join("\n\n"),
      requestedBy,
      requestedMaxTickets: boundedMaximum,
      model: "openai-codex/gpt-5.6-terra",
      thinking: "medium",
      agent: "backlog-splitter",
      tools: ["read", "grep", "find", "ls"],
      policy: "design-read-only",
      profile: "backlog-splitter",
      source: "backlog-draft",
      state: "queued",
      createdAt: now,
      history: [{ state: "queued", detail: `local backlog split requested by ${requestedBy}`, timestamp: now }],
      mailbox: [],
    };
  });
  await broadcast({ type: "backlog_split_queued", taskId });
  void schedule();
  return taskId;
}

async function admitTicketPipeline(ticket, prerequisiteTaskId) {
  if (!Number.isInteger(ticket) || ticket < 1) throw new Error("ticket must be a positive integer");
  const planId = `ticket-${ticket}-delivery`;
  const prerequisite = await withStateLock(statePath, (state) => {
    if (state.plans?.[planId]) throw new Error(`pipeline ${planId} is already admitted`);
    const task = state.tasks[prerequisiteTaskId];
    if (!task || task.state !== "done") throw new Error("pipeline prerequisite is not done");
    return { ...task };
  });
  const issueResult = await exec("gh", ["issue", "view", String(ticket), "-R", "Patrick-Kappen/graft", "--json", "number,title,body,state,labels,url"], { timeout: 15000 });
  if (issueResult.code !== 0) throw new Error(issueResult.stderr || `cannot read ticket #${ticket}`);
  const issue = JSON.parse(issueResult.stdout);
  if (issue.number !== ticket || issue.state !== "OPEN") throw new Error(`ticket #${ticket} is not open`);
  const baselineResult = await exec("git", ["rev-parse", "origin/main"], { timeout: 5000 });
  if (baselineResult.code !== 0 || !baselineResult.stdout.trim()) throw new Error("cannot resolve origin/main baseline");
  const baseline = baselineResult.stdout.trim();
  const stamp = Date.now();
  const branch = `orch-${ticket}-${stamp}`;
  const worktree = path.join("/tmp", `graft-orch-${ticket}-${stamp}`);
  const implementationId = `ticket-${ticket}-${stamp}-implementation`;
  const verificationId = `ticket-${ticket}-${stamp}-local-verification`;
  const created = await exec("git", ["worktree", "add", "-b", branch, worktree, baseline], { timeout: 30000 });
  if (created.code !== 0) throw new Error(created.stderr || "pipeline worktree creation failed");
  const acceptedReview = prerequisite.history?.filter((entry) => entry.state === "done").at(-1)?.detail ?? "Design accepted by orchestrator review.";
  const designResult = bounded(prerequisite.result?.finalText, 9000);
  const evidenceSessions = [prerequisite.sessionFile, ...(prerequisite.sessionHistory ?? [])].filter(Boolean).join("\n- ");
  await withStateLock(statePath, (state) => {
    state.plans ??= {};
    if (state.plans[planId]) throw new Error(`pipeline ${planId} was admitted concurrently`);
    const now = new Date().toISOString();
    state.tasks[implementationId] = {
      id: implementationId,
      ticket,
      phase: "implementation",
      pipelinePlanId: planId,
      dependsOn: [prerequisiteTaskId],
      worktree,
      branch,
      baseline,
      prompt: [
        `Approved implementation phase for GitHub #${ticket}: ${issue.title}`,
        issue.body ?? "",
        "Accepted architecture: implement the smallest Graft-owned notify wrapper around the real generated rootless Quadlet ExecStart. Preserve Type=notify, NotifyAccess=all, conmon MAINPID, ExitType=cgroup, restart/lifecycle semantics, authorization, manifests, and system-target behavior. Do not reconstruct Podman argv in Nix. Use a private credential-checked notify socket with a bounded 128-bit nonce path, exact-one canonical MAINPID, exact-or-slash-descendant cgroup validation, and NUL-delimited argv parity against systemd parsing.",
        `Binding orchestrator acceptance notes:\n${bounded(acceptedReview, 5000)}`,
        `Latest accepted design errata:\n${designResult}`,
        `Prior evidence sessions (read only if a detail is missing):\n- ${evidenceSessions || "none"}`,
        "Implement only this accepted scope in the assigned worktree. Add the focused parser/protocol/oracle/lifecycle tests and documentation required by the design. You may run repository-local builds and Nix VM tests, but never start/stop host systemd units or host containers. Do not commit, push, open a PR, change GitHub, tag, publish, or weaken assertions. Report changed files, exact commands, observable effects, and remaining unrun acceptance evidence.",
        "End exactly: STATE: done|blocked|failed — <one-sentence summary>.",
      ].join("\n\n"),
      model: "openai-codex/gpt-5.6-luna",
      thinking: "high",
      risk: "high",
      agent: "implementation-worker",
      tools: ["read", "bash", "edit", "write", "grep", "find", "ls"],
      policy: "implementation-worktree",
      profile: "implementation",
      source: "pipeline",
      preflight: { kind: "required-local-ci", maxAttempts: 3 },
      reviewDelayMs: 0,
      state: "queued",
      createdAt: now,
      history: [{ state: "queued", detail: `maintainer admitted ordered pipeline ${planId}`, timestamp: now }],
      mailbox: [],
    };
    state.tasks[verificationId] = {
      id: verificationId,
      ticket,
      phase: "local-verification",
      pipelinePlanId: planId,
      dependsOn: [implementationId],
      worktree,
      branch,
      baseline,
      prompt: [
        `Independent local-verification phase for GitHub #${ticket}: ${issue.title}`,
        issue.body ?? "",
        `Inspect the implementation diff from baseline ${baseline} after task ${implementationId} has passed orchestrator review. Do not modify tracked files. Verify relationships and observable effects, not command exit alone. Run the focused unit/Nix checks, normal and debug notify-protocol VM tests with their protocol controls, activation runtime coverage, and ten independent uninstrumented focused rootless VM executions when resources permit. Every counted execution must actually start and complete a VM. Capture bounded logs and exact derivation/SHA evidence. Runtime commands must execute inside Nix test VMs; never experiment against the host system/user manager or host containers. Do not commit, push, dispatch central CI, mutate GitHub, tag, or publish. Central runtime_tests=true remains an explicit external-action gate after local review.`,
        "End exactly: STATE: done|blocked|failed — <one-sentence summary>.",
      ].join("\n\n"),
      model: "opencode-go/deepseek-v4-flash",
      thinking: "low",
      risk: "high",
      agent: "verification-scout",
      tools: ["read", "bash", "grep", "find", "ls"],
      policy: "verification-worktree",
      profile: "verification",
      source: "pipeline",
      preflight: { kind: "required-local-ci", maxAttempts: 3 },
      reviewDelayMs: 0,
      state: "queued",
      createdAt: new Date(Date.parse(now) + 1).toISOString(),
      history: [{ state: "queued", detail: `ordered after ${implementationId}`, timestamp: now }],
      mailbox: [],
    };
    state.plans[planId] = {
      id: planId,
      ticket,
      title: issue.title,
      state: "running",
      prerequisiteTaskId,
      worktree,
      branch,
      baseline,
      phases: [
        { id: "design", taskId: prerequisiteTaskId },
        { id: "implementation", taskId: implementationId },
        { id: "local-verification", taskId: verificationId },
        { id: "central-ci", gate: "external-approval" },
        { id: "independent-go", gate: "maintainer" },
        { id: "release", gate: "external-approval" },
      ],
      createdAt: now,
      admittedBy: "maintainer",
    };
    state.admissions[String(ticket)] = {
      ticket,
      title: issue.title,
      body: issue.body ?? "",
      url: issue.url,
      profile: "ordered-pipeline",
      state: "admitted",
      planId,
      taskId: implementationId,
      discoveredAt: now,
      resolvedAt: now,
    };
  });
  await broadcast({ type: "pipeline_admitted", planId, ticket, implementationId, verificationId });
  void schedule();
  return { planId, implementationId, verificationId, worktree, branch, baseline };
}

async function waitForSocket(socketPath) {
  for (let attempt = 0; attempt < 100; attempt++) {
    if (await exists(socketPath)) return;
    await new Promise((resolve) => setTimeout(resolve, 50));
  }
  throw new Error("runner socket did not appear");
}

async function sendPrompt(socketPath, message) {
  return await new Promise((resolve, reject) => {
    const socket = net.createConnection(socketPath);
    let sent = false;
    const timer = setTimeout(() => { socket.destroy(); reject(new Error("runner prompt timeout")); }, 5000);
    attachJsonl(socket, (record) => {
      if (record.type === "runner_ready" && !sent) {
        sent = true;
        writeRecord(socket, { type: "prompt", id: randomUUID(), message });
      } else if (record.type === "pi_rpc_event" && record.event?.type === "response" && record.event.command === "prompt") {
        clearTimeout(timer);
        socket.end();
        resolve();
      }
    }, reject);
    socket.on("error", reject);
  });
}

async function launchTask(task) {
  const taskDir = path.join(runtimeRoot, "tasks", task.id);
  const socketPath = path.join(taskDir, "agent.sock");
  const configPath = path.join(taskDir, "runner.json");
  const sessionFile = task.nextSessionFile ?? task.sessionFile ?? path.join(stateRoot, "sessions", `${task.id}-${Date.now()}.jsonl`);
  await mkdir(taskDir, { recursive: true, mode: 0o700 });
  await rm(socketPath, { force: true });
  const config = {
    taskId: task.id,
    cwd: task.worktree,
    socketPath,
    controlSocket,
    eventLog,
    sessionFile,
    model: task.model,
    thinking: task.nextThinking ?? task.thinking,
    tools: task.tools,
    policy: task.policy,
    bridgePath: path.join(extensionRoot, "agent-bridge.ts"),
  };
  await writeFile(configPath, `${JSON.stringify(config)}\n`, { mode: 0o600 });
  const window = `orch-${task.id}`;
  const existing = await exec("tmux", ["has-session", "-t", `${tmuxSession}:${window}`], { timeout: 3000 });
  if (existing.code === 0) throw new Error(`runner window ${window} already exists`);
  const command = `exec ${JSON.stringify(process.execPath)} ${JSON.stringify(path.join(extensionRoot, "runner.mjs"))} --config ${JSON.stringify(configPath)}`;
  const launched = await exec("tmux", ["new-window", "-d", "-t", tmuxSession, "-n", window, command], { timeout: 5000 });
  if (launched.code !== 0) throw new Error(launched.stderr || "tmux runner launch failed");
  await waitForSocket(socketPath);
  await sendPrompt(socketPath, task.dispatchMessage);
  await withStateLock(statePath, (state) => {
    const current = state.tasks[task.id];
    if (current.sessionFile && current.sessionFile !== sessionFile) {
      current.sessionHistory ??= [];
      if (!current.sessionHistory.includes(current.sessionFile)) current.sessionHistory.push(current.sessionFile);
    }
    current.sessionFile = sessionFile;
    current.modelKey = current.model;
    current.thinkingLevel = task.nextThinking ?? task.thinking;
    current.runningSince = new Date().toISOString();
    current.dispatchingSince = undefined;
    current.nextSessionFile = undefined;
    current.nextThinking = undefined;
    transition(current, "running", `runner accepted ${current.resumeRoute ?? "fresh"} prompt`);
  });
  await broadcast({ type: "task_started", taskId: task.id });
}

async function schedule() {
  if (scheduling || stopping || pauseDispatch) return;
  scheduling = true;
  try {
    while (true) {
      const task = await withStateLock(statePath, (state) => {
        const candidate = nextEligible(state, concurrency);
        if (!candidate) return undefined;
        const message = candidate.mailbox?.shift() ?? candidate.prompt;
        candidate.dispatchMessage = message;
        candidate.dispatchingSince = new Date().toISOString();
        candidate.runningSince = undefined;
        transition(candidate, "dispatching", "daemon reserved eligible task");
        return { ...candidate };
      });
      if (!task) break;
      try {
        const githubSnapshot = Number.isInteger(task.ticket) ? await fetchIssueSnapshot(task.ticket) : undefined;
        if (githubSnapshot && githubSnapshot.issue.state !== "OPEN") throw new Error(`ticket #${task.ticket} is ${githubSnapshot.issue.state}`);
        const branch = await exec("git", ["-C", task.worktree, "branch", "--show-current"], { timeout: 5000 });
        const head = await exec("git", ["-C", task.worktree, "rev-parse", "HEAD"], { timeout: 5000 });
        if (branch.code !== 0 || branch.stdout.trim() !== task.branch) throw new Error("worktree branch drift");
        if (head.code !== 0) throw new Error("worktree HEAD unavailable");
        await withStateLock(statePath, (state) => {
          const current = state.tasks[task.id];
          if (githubSnapshot) current.githubSnapshot = githubSnapshot;
          current.verifiedHead = head.stdout.trim();
          transition(current, "dispatching", githubSnapshot ? `freshness gate passed at ${githubSnapshot.fetchedAt}` : "local read-only backlog draft gate passed");
        });
        await launchTask(task);
      } catch (error) {
        await withStateLock(statePath, (state) => {
          const current = state.tasks[task.id];
          if (current) transition(current, "failed", `dispatch failed: ${error instanceof Error ? error.message : String(error)}`);
        });
        await broadcast({ type: "task_failed", taskId: task.id, error: error instanceof Error ? error.message : String(error) });
      }
    }
  } finally {
    scheduling = false;
  }
}

function summarizeActivity(taskId, event) {
  const previous = activity.get(taskId) ?? {};
  const next = { ...previous, taskId, eventType: event.type, updatedAt: new Date().toISOString() };
  if (event.type === "tool_execution_start") {
    next.phase = "tool";
    next.tool = event.toolName;
    next.detail = JSON.stringify(event.args ?? {}).slice(0, 500);
  } else if (event.type === "message_update" && event.assistantMessageEvent?.type === "text_delta") {
    next.phase = "responding";
    next.text = `${next.text ?? ""}${event.assistantMessageEvent.delta}`.slice(-1000);
  } else if (event.type === "agent_start") next.phase = "running";
  else if (event.type === "agent_settled") next.phase = "settled";
  activity.set(taskId, next);
  return next;
}

async function performPreflight(taskId) {
  const task = await withStateLock(statePath, (state) => {
    const current = state.tasks[taskId];
    if (!current || current.state !== "scanning" || current.preflightRun?.state !== "running") return undefined;
    return { ...current, preflight: { ...current.preflight }, preflightRun: { ...current.preflightRun } };
  });
  if (!task) return;
  const attempt = task.preflightRun.attempt;
  let outcome;
  await acquireScanSlot();
  try {
    const scans = scanPlan(task.preflight.kind, { allowTest: process.env.GRAFT_ORCH_ALLOW_TEST_PREFLIGHT === "1" });
    outcome = await runScanPlan(
      scans,
      (scan) => executeScan(task, attempt, scan),
      async (progress) => {
        const running = progress.type === "scan_start";
        const scan = progress.scan;
        const previous = activity.get(taskId) ?? {};
        const next = {
          ...previous,
          taskId,
          eventType: progress.type,
          updatedAt: new Date().toISOString(),
          phase: "scanning",
          tool: "automatic checks",
          detail: running ? "running" : progress.result.code === 0 ? "green" : "red",
        };
        activity.set(taskId, next);
        await broadcast({ type: "activity", activity: next });
      },
    );
  } catch (error) {
    outcome = {
      passed: false,
      results: [{ id: "preflight-harness", label: "preflight harness", phase: "quick", code: 1, timedOut: false, logPath: undefined, stderr: error instanceof Error ? error.message : String(error), stdout: "" }],
    };
  } finally {
    releaseScanSlot();
  }
  let route;
  await withStateLock(statePath, (state) => {
    const current = state.tasks[taskId];
    if (!current || current.state !== "scanning" || current.preflightRun?.attempt !== attempt) return;
    const finishedAt = new Date().toISOString();
    const summary = {
      attempt,
      state: outcome.passed ? "green" : "red",
      startedAt: current.preflightRun.startedAt,
      finishedAt,
      scans: outcome.results.map((result) => ({
        id: result.id,
        label: result.label,
        phase: result.phase,
        code: result.code,
        timedOut: result.timedOut,
        logPath: result.logPath,
      })),
    };
    current.preflightRun = summary;
    current.preflightHistory ??= [];
    current.preflightHistory.push(summary);
    if (current.preflightHistory.length > 20) current.preflightHistory.splice(0, current.preflightHistory.length - 20);
    if (outcome.passed) {
      markReviewReady(current);
      if (legacyMainReview) {
        transition(current, "awaiting_review", `automatic preflight green (${summary.scans.length}/${summary.scans.length}); queued for legacy orchestrator review${current.reviewNotBefore ? ` after ${current.reviewNotBefore}` : ""}`);
        state.reviewQueue ??= [];
        if (!state.reviewQueue.includes(current.id)) state.reviewQueue.push(current.id);
        route = "green-legacy";
      } else if (current.reviewNotBefore) {
        transition(current, "review_deferred", `automatic preflight green (${summary.scans.length}/${summary.scans.length}); fresh review deferred until ${current.reviewNotBefore}`);
        route = "green-deferred";
      } else {
        transition(current, "preparing_review", `automatic preflight green (${summary.scans.length}/${summary.scans.length}); preparing fresh bounded review`);
        route = "green-fresh";
      }
      return;
    }
    const failed = summary.scans.filter((scan) => scan.code !== 0);
    const maximum = Math.max(1, Math.min(5, current.preflight?.maxAttempts ?? 3));
    if (attempt < maximum) {
      const now = Date.now();
      current.nextSessionFile = path.join(stateRoot, "sessions", `${current.id}-scan-repair-${now}.jsonl`);
      current.resumeRoute = "automatic-scan-repair";
      current.nextThinking = "low";
      current.mailbox ??= [];
      current.mailbox.push(freshFollowUpHandoff(current, [
        `Automatic preflight RED on attempt ${attempt}/${maximum}. This failure was not sent to the maintainer or orchestrator review.`,
        "Inspect and fix the private scan logs:",
        ...failed.map((scan) => `- ${scan.label}: ${scan.logPath ?? "preflight harness failure"}`),
        "Re-run the affected commands and the complete applicable suite before settling. Do not weaken or skip a scan.",
      ].join("\n")));
      transition(current, "queued", `automatic preflight red (${failed.map((scan) => scan.id).join(", ")}); returned privately to subagent`);
      route = "red-returned";
    } else {
      current.reviewNotBefore = undefined;
      transition(current, "awaiting_review", `automatic preflight remained red after ${attempt} private repair attempts; exceptional orchestrator review required`);
      state.reviewQueue ??= [];
      if (!state.reviewQueue.includes(current.id)) state.reviewQueue.push(current.id);
      route = "red-exhausted";
    }
  });
  if (route === "green-legacy") await broadcast({ type: "review_ready", taskId, preflight: "green" });
  else if (route === "green-fresh") await prepareFreshReview(taskId);
  else if (route === "green-deferred") await broadcast({ type: "fresh_review_deferred", taskId });
  else if (route === "red-returned") {
    await broadcast({ type: "preflight_red_returned", taskId });
    void schedule();
  } else if (route === "red-exhausted") await broadcast({ type: "review_ready", taskId, preflight: "red-exhausted" });
}

async function handleSettlement(event) {
  const eventId = event.eventId ?? `${event.taskId}:${event.timestamp}:${event.detail}`;
  let route;
  await withStateLock(statePath, (state) => {
    state.processedEvents ??= [];
    state.reviewQueue ??= [];
    if (state.processedEvents.includes(eventId)) return;
    state.processedEvents.push(eventId);
    if (state.processedEvents.length > 1024) state.processedEvents.splice(0, state.processedEvents.length - 1024);
    const task = state.tasks[event.taskId];
    if (!task) return;
    if (event.usage && (event.sessionFile || task.sessionFile)) {
      task.sessionUsage ??= {};
      task.sessionUsage[event.sessionFile ?? task.sessionFile] = event.usage;
      task.usage = taskUsageFromSessions(task);
    }
    if (task.profile === "backlog-splitter") {
      route = settleBacklogPlan(state, task, event);
      return;
    }
    if (task.profile === "change-review") {
      route = settleFreshReview(state, task, event);
      return;
    }
    task.result = event;
    task.lastSettledAt = event.timestamp;
    if (task.preflight?.kind) {
      const priorAttempt = task.preflightRun?.state === "red" ? task.preflightRun.attempt : 0;
      task.preflightRun = { state: "running", attempt: priorAttempt + 1, startedAt: new Date().toISOString(), scans: [] };
      transition(task, "scanning", "agent settled; automatic preflight scans started");
      route = "scanning";
    } else {
      markReviewReady(task);
      if (legacyMainReview) {
        transition(task, "awaiting_review", `agent settled; queued for legacy orchestrator review${task.reviewNotBefore ? ` after ${task.reviewNotBefore}` : ""}`);
        if (!state.reviewQueue.includes(task.id)) state.reviewQueue.push(task.id);
        route = "review-legacy";
      } else if (task.reviewNotBefore) {
        transition(task, "review_deferred", `agent settled; fresh review deferred until ${task.reviewNotBefore}`);
        route = "review-deferred";
      } else {
        transition(task, "preparing_review", "agent settled; preparing fresh bounded review");
        route = "review-fresh";
      }
    }
  });
  if (!event.usage) await refreshTaskUsage(event.taskId);
  if (route === "scanning") {
    await broadcast({ type: "preflight_started", taskId: event.taskId });
    void performPreflight(event.taskId);
  } else if (route === "review-legacy") await broadcast({ type: "review_ready", taskId: event.taskId });
  else if (route === "review-fresh" || route === "review-retry") await prepareFreshReview(route === "review-retry" ? (await withStateLock(statePath, (state) => state.tasks[event.taskId]?.parentTaskId)) : event.taskId);
  else if (route === "review-deferred") await broadcast({ type: "fresh_review_deferred", taskId: event.taskId });
  else if (["review-approved", "review-changes-requested", "review-decision", "review-orphaned"].includes(route)) {
    await broadcast({ type: route, taskId: event.taskId });
    void schedule();
  } else if (["backlog-ready", "backlog-invalid"].includes(route)) {
    await broadcast({ type: route, taskId: event.taskId });
  }
}

async function resumePreflights() {
  const snapshot = await loadSnapshot();
  for (const task of snapshot.tasks.filter((item) => item.state === "scanning" && item.preflightRun?.state === "running")) {
    void performPreflight(task.id);
  }
}

async function resumeFreshReviews() {
  if (legacyMainReview) return;
  const preparing = await withStateLock(statePath, (state) => {
    const ids = [];
    for (const task of Object.values(state.tasks)) {
      if (task.profile === "change-review") continue;
      if (task.state === "awaiting_review" && task.preflightRun?.state !== "red") {
        task.reviewClaimedAt = undefined;
        state.reviewQueue = (state.reviewQueue ?? []).filter((id) => id !== task.id);
        if (task.reviewNotBefore && Date.parse(task.reviewNotBefore) > Date.now()) {
          transition(task, "review_deferred", `migrated legacy review to fresh review deferred until ${task.reviewNotBefore}`);
        } else {
          task.reviewNotBefore = undefined;
          transition(task, "preparing_review", "migrated legacy review to fresh bounded review agent");
          ids.push(task.id);
        }
      } else if (task.state === "preparing_review") ids.push(task.id);
    }
    return ids;
  });
  for (const taskId of preparing) await prepareFreshReview(taskId);
}

async function pollEventLog() {
  try {
    const content = await readFile(eventLog, "utf8");
    if (eventOffset > content.length) eventOffset = 0;
    const next = content.slice(eventOffset);
    eventOffset = content.length;
    for (const line of next.split("\n")) {
      if (!line) continue;
      const event = JSON.parse(line);
      if (event.type === "orchestrator_agent_settled") await handleSettlement(event);
      else if (event.type === "orchestrator_input_requested" && event.taskId) {
        let changed = false;
        await withStateLock(statePath, (state) => {
          state.processedEvents ??= [];
          const eventId = event.eventId ?? `${event.taskId}:${event.timestamp}:${event.title}`;
          if (state.processedEvents.includes(eventId)) return;
          state.processedEvents.push(eventId);
          const task = state.tasks[event.taskId];
          if (!task) return;
          task.inputRequests ??= [];
          task.inputRequests.push(event);
          changed = true;
        });
        if (changed) await broadcast({ type: "orchestrator_input_ready", taskId: event.taskId });
      }
    }
  } catch (error) {
    if (error.code !== "ENOENT") console.error(error);
  }
}

async function socketAcceptsConnections(socketPath) {
  return await new Promise((resolve) => {
    const socket = net.createConnection(socketPath);
    const timer = setTimeout(() => {
      socket.destroy();
      resolve(false);
    }, 250);
    socket.once("connect", () => {
      clearTimeout(timer);
      socket.end();
      resolve(true);
    });
    socket.once("error", () => {
      clearTimeout(timer);
      resolve(false);
    });
  });
}

async function reconcileRunning() {
  const snapshot = await loadSnapshot();
  for (const task of snapshot.tasks.filter((item) => ["running", "dispatching"].includes(item.state))) {
    const started = Date.parse((task.state === "dispatching" ? task.dispatchingSince : task.runningSince) ?? task.history?.at(-1)?.timestamp ?? "");
    if (!Number.isFinite(started) || Date.now() - started < runnerGraceMs) continue;
    const socketPath = path.join(runtimeRoot, "tasks", task.id, "agent.sock");
    const socketAlive = await socketAcceptsConnections(socketPath);
    const pane = await exec("tmux", ["has-session", "-t", `${tmuxSession}:orch-${task.id}`], { timeout: 3000 });
    if (socketAlive || pane.code === 0) continue;
    await rm(socketPath, { force: true });
    await withStateLock(statePath, (state) => {
      const current = state.tasks[task.id];
      if (current && ["running", "dispatching"].includes(current.state)) transition(current, "failed", "runner disappeared without settlement");
    });
    await broadcast({ type: "task_failed", taskId: task.id, error: "runner disappeared" });
  }
}

function markReviewReady(task) {
  const delay = Math.max(0, Math.min(60 * 60_000, task.reviewDelayMs ?? 0));
  task.reviewNotBefore = delay > 0 ? new Date(Date.now() + delay).toISOString() : undefined;
}

function reviewIsDue(task, now = Date.now()) {
  if (!task || task.state !== "awaiting_review" || task.reviewClaimedAt) return false;
  if (!task.reviewNotBefore) return true;
  const due = Date.parse(task.reviewNotBefore);
  return !Number.isFinite(due) || due <= now;
}

async function announceDueReviews() {
  const snapshot = await loadSnapshot();
  for (const task of snapshot.tasks.filter((item) => item.state === "review_deferred" && (!item.reviewNotBefore || Date.parse(item.reviewNotBefore) <= Date.now()))) {
    const ready = await withStateLock(statePath, (state) => {
      const current = state.tasks[task.id];
      if (!current || current.state !== "review_deferred" || (current.reviewNotBefore && Date.parse(current.reviewNotBefore) > Date.now())) return false;
      current.reviewNotBefore = undefined;
      transition(current, "preparing_review", "bounded review defer window elapsed");
      return true;
    });
    if (ready) await prepareFreshReview(task.id);
  }
  if (snapshot.tasks.some((task) => reviewIsDue(task))) {
    for (const socket of subscribers) writeRecord(socket, { type: "review_due", timestamp: new Date().toISOString() });
  }
}

async function claimReview() {
  return await withStateLock(statePath, (state) => {
    state.reviewQueue ??= [];
    const now = Date.now();
    const id = state.reviewQueue.find((taskId) => reviewIsDue(state.tasks[taskId], now));
    if (!id) return null;
    const task = state.tasks[id];
    task.reviewClaimedAt = new Date().toISOString();
    return { ...task };
  });
}

function bounded(text, max) {
  const value = String(text ?? "").trim();
  if (value.length <= max) return value;
  return `${value.slice(0, max)}\n[…truncated by orchestrator harness…]`;
}

async function buildFreshReviewBundle(task, attempt) {
  const reviewDir = path.join(stateRoot, "reviews", task.id);
  const bundlePath = path.join(reviewDir, `${attempt}-changes.md`);
  await mkdir(reviewDir, { recursive: true, mode: 0o700 });
  const status = await exec("git", ["-C", task.worktree, "status", "--short", "--untracked-files=all"], { timeout: 15000 });
  if (status.code !== 0) throw new Error(status.stderr || "cannot collect review status");
  let patch = "(no baseline was supplied; review the bounded deliverable below)";
  if (task.baseline) {
    const diff = await exec("git", ["-C", task.worktree, "diff", "--no-ext-diff", "--find-renames", "--find-copies", task.baseline, "--"], { timeout: 30000 });
    if (diff.code !== 0) throw new Error(diff.stderr || "cannot collect review diff");
    patch = diff.stdout || "(tracked diff is empty)";
    const untracked = await exec("git", ["-C", task.worktree, "ls-files", "--others", "--exclude-standard", "-z"], { timeout: 15000 });
    if (untracked.code !== 0) throw new Error(untracked.stderr || "cannot list untracked review files");
    const paths = untracked.stdout.split("\0").filter(Boolean).slice(0, 128);
    for (const relative of paths) {
      if (patch.length >= 220_000) break;
      const added = await exec("git", ["-C", task.worktree, "diff", "--no-index", "--", "/dev/null", relative], { timeout: 10000 });
      if (![0, 1].includes(added.code)) throw new Error(added.stderr || `cannot collect untracked diff for ${relative}`);
      patch += `\n${added.stdout}`;
    }
  }
  const scanSummary = task.preflightRun?.state === "green"
    ? task.preflightRun.scans.map((scan) => `- ${scan.label}: green`).join("\n")
    : "- deterministic preflight was not configured for this deliverable";
  const deliverable = ["implementation", "integration"].includes(task.profile)
    ? "(implementation narrative intentionally excluded; review the patch itself)"
    : bounded(task.result?.finalText, 16_000) || "(no bounded deliverable text was persisted)";
  const bundle = [
    "# Fresh independent review bundle",
    "This bundle is generated for a new context. Do not inspect prior Pi sessions or orchestrator conversation history.",
    `Candidate task: ${task.id}`,
    `Ticket: #${task.ticket}`,
    `Worktree: ${task.worktree}`,
    `Baseline: ${task.baseline ?? "not applicable"}`,
    "## Bounded acceptance scope",
    bounded(task.prompt, 16_000),
    "## Worktree status",
    status.stdout || "(clean according to git status)",
    "## Deterministic evidence",
    scanSummary,
    "## Bounded deliverable evidence",
    deliverable,
    "## Candidate changes",
    bounded(patch, 220_000),
  ].join("\n\n");
  await writeFile(bundlePath, `${bundle}\n`, { mode: 0o600 });
  return bundlePath;
}

async function prepareFreshReview(taskId) {
  const candidate = await withStateLock(statePath, (state) => {
    const task = state.tasks[taskId];
    if (!task || task.state !== "preparing_review") return undefined;
    return { ...task, preflightRun: task.preflightRun ? { ...task.preflightRun, scans: [...(task.preflightRun.scans ?? [])] } : undefined };
  });
  if (!candidate) return undefined;
  const attempt = (candidate.reviewAttempts ?? 0) + 1;
  let bundlePath;
  try {
    bundlePath = await buildFreshReviewBundle(candidate, attempt);
  } catch (error) {
    await withStateLock(statePath, (state) => {
      const task = state.tasks[taskId];
      if (task?.state === "preparing_review") transition(task, "failed", `fresh review bundle failed: ${error instanceof Error ? error.message : String(error)}`);
    });
    await broadcast({ type: "task_failed", taskId, error: `fresh review bundle failed: ${error instanceof Error ? error.message : String(error)}` });
    return undefined;
  }
  const reviewId = await withStateLock(statePath, (state) => {
    const parent = state.tasks[taskId];
    if (!parent || parent.state !== "preparing_review") return undefined;
    const id = `${taskId}-review-${attempt}`;
    if (state.tasks[id]) throw new Error(`fresh review task ${id} already exists`);
    const now = new Date().toISOString();
    parent.reviewAttempts = attempt;
    parent.reviewTaskId = id;
    parent.reviewNotBefore = undefined;
    transition(parent, "waiting_review_agent", `fresh bounded Sol/medium review ${id} queued`);
    state.tasks[id] = {
      id,
      ticket: parent.ticket,
      phase: `${parent.phase ?? parent.profile ?? "task"}-review`,
      parentTaskId: parent.id,
      dependsOn: [],
      worktree: parent.worktree,
      branch: parent.branch,
      baseline: parent.baseline,
      prompt: [
        `Fresh independent review for candidate ${parent.id}, GitHub #${parent.ticket}.`,
        `Read the generated bundle first: ${bundlePath}`,
        "Review only the bounded acceptance scope, deterministic evidence, candidate diff, and current versions of changed files. Do not inspect prior Pi sessions, mailboxes, conversation history, or unrelated work. Do not modify files or run shell commands. Identify concrete correctness, security, lifecycle, compatibility, and missing-test defects; avoid speculative style comments.",
        "Return concise findings ordered by severity with exact file/line references where possible.",
        "End with exactly one final line: REVIEW: approve — <reason>, REVIEW: changes_requested — <reason>, or REVIEW: blocked — <reason>.",
      ].join("\n\n"),
      reviewBundlePath: bundlePath,
      model: "openai-codex/gpt-5.6-sol",
      thinking: "medium",
      agent: "fresh-change-reviewer",
      tools: ["read", "grep", "find", "ls"],
      policy: "review-changes-read-only",
      profile: "change-review",
      source: "internal-review",
      state: "queued",
      createdAt: now,
      history: [{ state: "queued", detail: `fresh review of ${parent.id}; no prior session context`, timestamp: now }],
      mailbox: [],
    };
    return id;
  });
  if (reviewId) {
    await broadcast({ type: "fresh_review_queued", taskId, reviewTaskId: reviewId });
    void schedule();
  }
  return reviewId;
}

const backlogLabels = new Set(["status:backlog", "priority:P1", "type:bug", "type:chore", "type:design", "type:docs", "type:feature", "type:quality"]);

function parseBacklogPlan(text, maximum) {
  const value = String(text ?? "");
  const startMarker = "BACKLOG_PLAN_JSON";
  const endMarker = "END_BACKLOG_PLAN";
  const start = value.lastIndexOf(startMarker);
  const end = value.lastIndexOf(endMarker);
  if (start < 0 || end <= start) throw new Error("backlog splitter omitted the required JSON markers");
  const plan = JSON.parse(value.slice(start + startMarker.length, end).trim());
  if (!plan || typeof plan !== "object" || !Array.isArray(plan.tickets) || plan.tickets.length < 1 || plan.tickets.length > maximum) {
    throw new Error(`backlog plan must contain 1..${maximum} tickets`);
  }
  const ids = new Set();
  const tickets = plan.tickets.map((ticket, index) => {
    if (!ticket || typeof ticket !== "object") throw new Error(`ticket ${index + 1} is not an object`);
    const id = String(ticket.id ?? "");
    if (!/^[a-z0-9][a-z0-9-]{0,39}$/.test(id) || ids.has(id)) throw new Error(`ticket ${index + 1} has an invalid or duplicate id`);
    ids.add(id);
    const title = String(ticket.title ?? "").trim();
    const body = String(ticket.body ?? "").trim();
    if (!title || title.length > 160) throw new Error(`ticket ${id} title must contain 1..160 characters`);
    if (!body || body.length > 20_000) throw new Error(`ticket ${id} body must contain 1..20000 characters`);
    const labels = [...new Set(["status:backlog", ...(Array.isArray(ticket.labels) ? ticket.labels.map(String) : [])])];
    if (labels.some((label) => !backlogLabels.has(label))) throw new Error(`ticket ${id} contains an unsupported label`);
    if (labels.filter((label) => label.startsWith("type:")).length > 1) throw new Error(`ticket ${id} contains multiple type labels`);
    const dependsOn = [...new Set(Array.isArray(ticket.dependsOn) ? ticket.dependsOn.map(String) : [])];
    return { id, title, body, labels, dependsOn, originalIndex: index };
  });
  for (const ticket of tickets) {
    if (ticket.dependsOn.includes(ticket.id) || ticket.dependsOn.some((id) => !ids.has(id))) throw new Error(`ticket ${ticket.id} has an invalid dependency`);
  }
  const ordered = [];
  const remaining = new Map(tickets.map((ticket) => [ticket.id, ticket]));
  while (remaining.size) {
    const ready = [...remaining.values()].filter((ticket) => ticket.dependsOn.every((id) => ordered.some((done) => done.id === id))).sort((a, b) => a.originalIndex - b.originalIndex);
    if (!ready.length) throw new Error("backlog plan dependency graph contains a cycle");
    for (const ticket of ready) {
      ordered.push(ticket);
      remaining.delete(ticket.id);
    }
  }
  return {
    title: bounded(plan.title, 200) || "Backlog decomposition",
    summary: bounded(plan.summary, 3000),
    tickets: ordered.map(({ originalIndex: _originalIndex, ...ticket }) => ticket),
  };
}

function settleBacklogPlan(state, task, event) {
  task.result = event;
  task.lastSettledAt = event.timestamp;
  let plan;
  try {
    plan = parseBacklogPlan(event.finalText, task.requestedMaxTickets ?? 6);
  } catch (error) {
    transition(task, "failed", `backlog split was not valid: ${error instanceof Error ? error.message : String(error)}`);
    return "backlog-invalid";
  }
  const draftId = `backlog-${task.id}`;
  const now = new Date().toISOString();
  state.backlogDrafts ??= {};
  state.backlogDrafts[draftId] = {
    id: draftId,
    taskId: task.id,
    title: plan.title,
    summary: plan.summary,
    tickets: plan.tickets,
    state: "pending_approval",
    createdAt: now,
    requestedBy: task.requestedBy,
    createdIssues: [],
  };
  task.backlogDraftId = draftId;
  transition(task, "done", `backlog split prepared ${plan.tickets.length} ordered ticket drafts for maintainer approval`);
  const lines = plan.tickets.map((ticket, index) => `${index + 1}. ${ticket.title}${ticket.dependsOn.length ? ` (after ${ticket.dependsOn.join(", ")})` : ""}`).join("\n");
  const decisionId = randomUUID();
  state.decisions[decisionId] = {
    id: decisionId,
    taskId: task.id,
    draftId,
    kind: "backlog_create",
    title: `Create ${plan.tickets.length} backlog ticket${plan.tickets.length === 1 ? "" : "s"}?`,
    question: [`Approve creating exactly this ordered GitHub backlog batch?`, plan.summary, lines, "Yes authorizes only creation of these issues with the displayed bodies, labels, and dependency links; it does not approve implementation, push, PR, merge, or release."].filter(Boolean).join("\n\n"),
    state: "pending",
    createdAt: now,
  };
  return "backlog-ready";
}

function parseFreshReviewVerdict(text) {
  const line = String(text ?? "").trim().split("\n").at(-1)?.trim() ?? "";
  const match = /^REVIEW:\s*(approve|changes_requested|blocked)\s+(?:—|-)\s+(.+)$/i.exec(line);
  return match ? { verdict: match[1].toLowerCase(), reason: match[2].trim() } : undefined;
}

function settleFreshReview(state, reviewTask, event) {
  const parent = state.tasks[reviewTask.parentTaskId];
  if (!parent || parent.reviewTaskId !== reviewTask.id) {
    transition(reviewTask, "failed", "fresh review parent is unavailable or no longer expects this review");
    return "review-orphaned";
  }
  reviewTask.result = event;
  reviewTask.lastSettledAt = event.timestamp;
  const parsed = parseFreshReviewVerdict(event.finalText);
  if (!parsed) {
    transition(reviewTask, "failed", "fresh reviewer omitted the required terminal REVIEW verdict");
    parent.reviewTaskId = undefined;
    if ((parent.reviewAttempts ?? 0) < 2) {
      transition(parent, "preparing_review", "malformed fresh review verdict; retrying once in another fresh session");
      return "review-retry";
    }
    const decisionId = randomUUID();
    state.decisions[decisionId] = {
      id: decisionId,
      taskId: parent.id,
      kind: "decision",
      title: "Fresh review could not produce a verdict",
      question: `Two isolated review attempts for ${parent.id} failed to produce a typed verdict. Retry the candidate work?`,
      state: "pending",
      createdAt: new Date().toISOString(),
    };
    transition(parent, "waiting_decision", "fresh review harness exhausted its bounded retry");
    return "review-decision";
  }
  reviewTask.reviewVerdict = parsed.verdict;
  reviewTask.reviewReason = parsed.reason;
  transition(reviewTask, "done", `fresh review verdict: ${parsed.verdict} — ${parsed.reason}`);
  parent.reviewResult = event;
  parent.reviewedAt = new Date().toISOString();
  parent.reviewTaskId = undefined;
  if (parsed.verdict === "approve") {
    transition(parent, "done", `fresh Sol/medium review approved changes: ${parsed.reason}`);
    updatePipelineAfterReview(state, parent, "complete");
    return "review-approved";
  }
  if (parsed.verdict === "changes_requested") {
    parent.preflightRun = undefined;
    const now = Date.now();
    parent.nextSessionFile = path.join(stateRoot, "sessions", `${parent.id}-review-repair-${now}.jsonl`);
    parent.resumeRoute = "fresh-review-repair";
    parent.mailbox ??= [];
    parent.mailbox.push(freshFollowUpHandoff(parent, [
      "A fresh independent Sol/medium review inspected only the bounded candidate changes and requested corrections.",
      bounded(event.finalText, 12_000),
      "Fix only concrete review findings, then rerun the complete applicable checks before settling.",
    ].join("\n\n")));
    transition(parent, "queued", `fresh review requested changes: ${parsed.reason}`);
    return "review-changes-requested";
  }
  const decisionId = randomUUID();
  state.decisions[decisionId] = {
    id: decisionId,
    taskId: parent.id,
    kind: "decision",
    title: "Fresh review is blocked",
    question: `Fresh isolated review of ${parent.id} is blocked: ${parsed.reason}`,
    state: "pending",
    createdAt: new Date().toISOString(),
  };
  transition(parent, "waiting_decision", `fresh review blocked: ${parsed.reason}`);
  return "review-decision";
}

function freshFollowUpHandoff(task, instruction) {
  const priorSummary = task.result?.finalText
    ? bounded(task.result.finalText.split("\n").slice(-24).join("\n"), 4000)
    : "No prior result was persisted.";
  return [
    "[Fresh orchestrator handoff — do not reconstruct the full prior chat unless strictly necessary]",
    `Logical agent: ${task.agent ?? task.profile ?? "worker"}`,
    `Model route: ${task.model ?? "default"}:${task.nextThinking ?? task.thinking ?? "default"}`,
    `Ticket: #${task.ticket}`,
    `Task: ${task.id}`,
    `Worktree: ${task.worktree}`,
    `Branch: ${task.branch}`,
    `Baseline: ${task.baseline ?? task.verifiedHead ?? "unknown"}`,
    `Previous session (evidence only): ${task.sessionFile ?? "none"}`,
    `Original scope:\n${bounded(task.prompt, 5000)}`,
    `Prior result tail:\n${priorSummary}`,
    `Reviewer instruction:\n${instruction.trim()}`,
    "Inspect the current worktree and cited evidence directly. Remain within the original mutation policy. End with the required STATE line.",
  ].join("\n\n");
}

function updatePipelineAfterReview(state, task, action) {
  if (!task.pipelinePlanId || !["complete", "blocked"].includes(action)) return;
  const plan = state.plans?.[task.pipelinePlanId];
  if (!plan) return;
  const phase = plan.phases?.find((item) => item.taskId === task.id);
  if (phase) phase.state = action === "complete" ? "done" : "blocked";
  plan.updatedAt = new Date().toISOString();
  if (action === "blocked") {
    plan.state = "blocked";
    for (const dependent of Object.values(state.tasks)) {
      if (dependent.state === "queued" && dependent.dependsOn?.includes(task.id)) {
        transition(dependent, "blocked", `dependency ${task.id} was blocked`);
      }
    }
    return;
  }
  if (task.phase !== "local-verification") return;
  plan.state = "waiting_external_approval";
  if (Object.values(state.decisions).some((decision) => decision.planId === plan.id && decision.kind === "external_ci" && decision.state === "pending")) return;
  const id = randomUUID();
  state.decisions[id] = {
    id,
    taskId: task.id,
    planId: plan.id,
    kind: "external_ci",
    title: "Central runtime CI approval required",
    question: `Local implementation and verification for #${plan.ticket} passed review. Approve dispatching the central workflow with runtime_tests=true on exact SHA ${task.verifiedHead ?? plan.baseline}? This approval does not approve a PR, merge, tag, or release.`,
    state: "pending",
    createdAt: new Date().toISOString(),
  };
}

async function createBacklogIssues(draftId) {
  const draft = await withStateLock(statePath, (state) => {
    const current = state.backlogDrafts?.[draftId];
    if (!current || current.state !== "creating") return undefined;
    return structuredClone(current);
  });
  if (!draft) return;
  const createdById = new Map((draft.createdIssues ?? []).map((item) => [item.planId, item]));
  try {
    for (const ticket of draft.tickets) {
      if (createdById.has(ticket.id)) continue;
      const dependencies = ticket.dependsOn.map((id) => createdById.get(id)).filter(Boolean);
      if (dependencies.length !== ticket.dependsOn.length) throw new Error(`dependency issue numbers for ${ticket.id} are incomplete`);
      const marker = `<!-- graft-backlog-draft:${draft.id}:${ticket.id} -->`;
      const dependencySection = dependencies.length
        ? `\n\n## Dependencies\n${dependencies.map((item) => `- Depends on #${item.number}`).join("\n")}`
        : "\n\n## Dependencies\n- None";
      const body = `${ticket.body}${dependencySection}\n\n${marker}\n`;
      const bodyDir = path.join(stateRoot, "backlog", draft.id);
      const bodyPath = path.join(bodyDir, `${ticket.id}.md`);
      await mkdir(bodyDir, { recursive: true, mode: 0o700 });
      await writeFile(bodyPath, body, { mode: 0o600 });
      const argv = ["issue", "create", "-R", "Patrick-Kappen/graft", "--title", ticket.title, "--body-file", bodyPath];
      for (const label of ticket.labels) argv.push("--label", label);
      const result = await exec("gh", argv, { timeout: 30000 });
      if (result.code !== 0) throw new Error(result.stderr || `GitHub issue creation failed for ${ticket.id}`);
      const url = result.stdout.trim().split("\n").filter(Boolean).at(-1) ?? "";
      const match = /\/issues\/(\d+)(?:$|[?#])/.exec(url);
      if (!match) throw new Error(`GitHub did not return an issue URL for ${ticket.id}`);
      const created = { planId: ticket.id, number: Number.parseInt(match[1], 10), url, title: ticket.title, createdAt: new Date().toISOString() };
      createdById.set(ticket.id, created);
      await withStateLock(statePath, (state) => {
        const current = state.backlogDrafts[draft.id];
        current.createdIssues ??= [];
        if (!current.createdIssues.some((item) => item.planId === ticket.id)) current.createdIssues.push(created);
        current.updatedAt = new Date().toISOString();
      });
      await broadcast({ type: "backlog_issue_created", draftId: draft.id, issue: created });
    }
    await withStateLock(statePath, (state) => {
      const current = state.backlogDrafts[draft.id];
      current.state = "created";
      current.completedAt = new Date().toISOString();
    });
    await broadcast({ type: "backlog_batch_created", draftId: draft.id });
  } catch (error) {
    await withStateLock(statePath, (state) => {
      const current = state.backlogDrafts[draft.id];
      current.state = "partial_failed";
      current.error = error instanceof Error ? error.message : String(error);
      current.updatedAt = new Date().toISOString();
      const id = randomUUID();
      state.decisions[id] = {
        id,
        taskId: current.taskId,
        draftId: current.id,
        kind: "decision",
        title: "Backlog batch creation needs inspection",
        question: `Backlog batch ${current.id} stopped after ${current.createdIssues?.length ?? 0}/${current.tickets.length} issues: ${current.error}. Inspect GitHub before deciding whether to draft the remainder again; the daemon will not retry automatically.`,
        state: "pending",
        createdAt: new Date().toISOString(),
      };
    });
    await broadcast({ type: "backlog_batch_failed", draftId: draft.id, error: error instanceof Error ? error.message : String(error) });
  }
}

async function reviewAction(taskId, action, message) {
  await withStateLock(statePath, (state) => {
    const task = state.tasks[taskId];
    if (!task || task.state !== "awaiting_review") throw new Error("task is not awaiting review");
    task.reviewClaimedAt = undefined;
    task.reviewNotBefore = undefined;
    task.reviewedAt = new Date().toISOString();
    state.reviewQueue = (state.reviewQueue ?? []).filter((id) => id !== taskId);
    if (action === "follow_up") {
      if (!message?.trim()) throw new Error("follow_up requires a message");
      task.preflightRun = undefined;
      task.nextThinking = task.profile === "design" ? "medium" : task.risk === "high" ? "high" : "low";
      const now = Date.now();
      task.nextSessionFile = path.join(stateRoot, "sessions", `${task.id}-followup-${now}.jsonl`);
      task.resumeRoute = "fresh-handoff";
      task.mailbox.push(freshFollowUpHandoff(task, message));
      transition(task, "queued", "orchestrator queued fresh bounded follow-up handoff");
    } else if (action === "ask_maintainer" || action === "request_pr") {
      if (!message?.trim()) throw new Error(`${action} requires a question`);
      const id = randomUUID();
      state.decisions[id] = { id, taskId, kind: action === "request_pr" ? "pr" : "decision", title: action === "request_pr" ? "PR approval requested" : "Orchestrator decision requested", question: message.trim(), state: "pending", createdAt: new Date().toISOString() };
      transition(task, action === "request_pr" ? "waiting_pr_approval" : "waiting_decision", message.trim());
    } else if (action === "complete") transition(task, "done", message?.trim() || "orchestrator accepted result");
    else if (action === "blocked") transition(task, "blocked", message?.trim() || "orchestrator marked blocked");
    else throw new Error(`unknown review action ${action}`);
    updatePipelineAfterReview(state, task, action);
  });
  await broadcast({ type: "review_resolved", taskId, action });
  void schedule();
}

async function retryFailedTask(taskId, reason) {
  await withStateLock(statePath, (state) => {
    const task = state.tasks[taskId];
    if (!task || task.state !== "failed") throw new Error("task is not failed");
    if (!task.dispatchMessage?.trim()) throw new Error("failed task has no durable handoff to retry");
    const now = Date.now();
    task.nextSessionFile = path.join(stateRoot, "sessions", `${task.id}-retry-${now}.jsonl`);
    task.resumeRoute = "fresh-handoff-retry";
    task.mailbox.push(task.dispatchMessage);
    transition(task, "queued", reason?.trim() || "daemon queued bounded retry after harness recovery");
  });
  await broadcast({ type: "task_retried", taskId });
  void schedule();
}

async function resolveDecision(id, answer, explanation) {
  let backlogToCreate;
  await withStateLock(statePath, (state) => {
    const decision = state.decisions[id];
    if (!decision || decision.state !== "pending") throw new Error("decision is not pending");
    const task = state.tasks[decision.taskId];
    decision.state = "resolved";
    decision.answer = answer;
    decision.explanation = explanation || undefined;
    decision.resolvedAt = new Date().toISOString();
    if (decision.kind === "backlog_create") {
      const draft = state.backlogDrafts?.[decision.draftId];
      if (!draft || draft.state !== "pending_approval") throw new Error("backlog draft is not pending approval");
      draft.state = answer === "yes" ? "creating" : "rejected";
      draft.approvalAnswer = answer;
      draft.approvalExplanation = explanation || undefined;
      draft.approvedAt = answer === "yes" ? new Date().toISOString() : undefined;
      draft.updatedAt = new Date().toISOString();
      if (answer === "yes") backlogToCreate = draft.id;
    } else if (decision.kind === "pr") {
      if (answer !== "yes") task.reviewNotBefore = undefined;
      transition(task, answer === "yes" ? "waiting_pr_action" : "awaiting_review", answer === "yes" ? "maintainer approved PR action" : "maintainer rejected PR action");
      if (answer !== "yes" && !state.reviewQueue.includes(task.id)) state.reviewQueue.push(task.id);
    } else if (decision.kind === "external_ci") {
      const plan = state.plans?.[decision.planId];
      if (!plan) throw new Error("decision pipeline is unavailable");
      plan.state = answer === "yes" ? "waiting_external_action" : "paused";
      plan.externalCiAnswer = answer;
      plan.externalCiExplanation = explanation || undefined;
      plan.updatedAt = new Date().toISOString();
    } else {
      if (!task) throw new Error("decision task is unavailable");
      task.mailbox.push(`Maintainer answer: ${answer}${explanation ? ` — ${explanation}` : ""}`);
      transition(task, "queued", "maintainer response queued for agent");
    }
  });
  await broadcast({ type: "decision_resolved", id, answer });
  if (backlogToCreate) void createBacklogIssues(backlogToCreate);
  void schedule();
}

async function handleCommand(socket, command) {
  const id = command.id;
  try {
    let data;
    if (command.type === "ping") data = { pid: process.pid };
    else if (command.type === "snapshot") data = await loadSnapshot();
    else if (command.type === "subscribe") {
      subscribers.add(socket);
      data = await loadSnapshot();
    } else if (command.type === "refresh_admissions") { await syncAdmissions(); data = true; }
    else if (command.type === "draft_backlog") data = { taskId: await draftBacklog(command.request, command.maxTickets, command.requestedBy) };
    else if (command.type === "approve_admission") data = { taskId: await approveAdmission(command.ticket) };
    else if (command.type === "admit_pipeline") data = await admitTicketPipeline(command.ticket, command.prerequisiteTaskId);
    else if (command.type === "park_admission") {
      await withStateLock(statePath, (state) => {
        const item = state.admissions[String(command.ticket)];
        if (!item || item.state !== "pending") throw new Error("admission not pending");
        item.state = "parked";
        item.explanation = command.explanation?.trim() || undefined;
        item.resolvedAt = new Date().toISOString();
      });
      await broadcast({ type: "admission_parked", ticket: command.ticket }); data = true;
    } else if (command.type === "claim_review") {
      data = await claimReview();
      if (data) await broadcast({ type: "review_claimed", taskId: data.id });
    } else if (command.type === "review_action") { await reviewAction(command.taskId, command.action, command.message); data = true; }
    else if (command.type === "retry_failed") { await retryFailedTask(command.taskId, command.reason); data = true; }
    else if (command.type === "resolve_decision") { await resolveDecision(command.decisionId, command.answer, command.explanation); data = true; }
    else if (command.type === "runner_event") {
      const next = summarizeActivity(command.taskId, command.event ?? {});
      await broadcast({ type: "activity", activity: next }); data = true;
    } else throw new Error(`unknown command ${command.type}`);
    writeRecord(socket, { type: "response", id, success: true, data });
  } catch (error) {
    writeRecord(socket, { type: "response", id, success: false, error: error instanceof Error ? error.message : String(error) });
  }
}

await mkdir(runtimeRoot, { recursive: true, mode: 0o700 });
await mkdir(stateRoot, { recursive: true, mode: 0o700 });
if (await exists(controlSocket)) {
  const live = await new Promise((resolve) => {
    const probe = net.createConnection(controlSocket);
    probe.once("connect", () => { probe.end(); resolve(true); });
    probe.once("error", () => resolve(false));
  });
  if (live) throw new Error("orchestrator daemon is already running");
  await rm(controlSocket, { force: true });
}
await migrate();
const server = net.createServer((socket) => {
  attachJsonl(socket, (command) => void handleCommand(socket, command), (error) => writeRecord(socket, { type: "protocol_error", error: error.message }));
  socket.on("close", () => subscribers.delete(socket));
  socket.on("error", () => subscribers.delete(socket));
});
server.listen(controlSocket, async () => {
  await chmod(controlSocket, 0o600);
  console.error(`graft orchestrator daemon ready: ${controlSocket}`);
  await syncAdmissions().catch((error) => console.error(`admission refresh: ${error.message}`));
  await broadcast({ type: "daemon_ready", pid: process.pid });
  await resumePreflights();
  await resumeFreshReviews();
  void schedule();
});
setInterval(() => void pollEventLog(), 250).unref();
setInterval(() => void reconcileRunning(), 1000).unref();
setInterval(() => void announceDueReviews().catch((error) => console.error(`review due check: ${error.message}`)), 5000).unref();
setInterval(() => void syncAdmissions().catch((error) => console.error(`admission refresh: ${error.message}`)), 60000).unref();
process.on("SIGTERM", () => {
  stopping = true;
  server.close();
  for (const socket of subscribers) socket.end();
  for (const child of scanProcesses.values()) child.kill("SIGTERM");
  void rm(controlSocket, { force: true }).finally(() => process.exit(0));
});
