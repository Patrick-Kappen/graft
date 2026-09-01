import { mkdir, open, readFile, rename, rm, writeFile } from "node:fs/promises";
import { dirname, join } from "node:path";

const VERSION = 1;

function emptyState() {
  return { version: VERSION, tasks: {}, decisions: {}, admissions: {}, plans: {}, backlogDrafts: {} };
}

export async function loadState(statePath) {
  try {
    const state = JSON.parse(await readFile(statePath, "utf8"));
    if (state?.version !== VERSION || !state.tasks || typeof state.tasks !== "object") {
      throw new Error("unsupported orchestrator state format");
    }
    if (!state.decisions || typeof state.decisions !== "object") state.decisions = {};
    if (!state.admissions || typeof state.admissions !== "object") state.admissions = {};
    if (!state.plans || typeof state.plans !== "object") state.plans = {};
    if (!state.backlogDrafts || typeof state.backlogDrafts !== "object") state.backlogDrafts = {};
    return state;
  } catch (error) {
    if (error?.code === "ENOENT") return emptyState();
    throw error;
  }
}

export async function saveState(statePath, state) {
  await mkdir(dirname(statePath), { recursive: true, mode: 0o700 });
  const temporary = `${statePath}.${process.pid}.${Date.now()}.tmp`;
  await writeFile(temporary, `${JSON.stringify(state, null, 2)}\n`, { encoding: "utf8", mode: 0o600 });
  await rename(temporary, statePath);
}

export async function withStateLock(statePath, action) {
  const lockPath = `${statePath}.lock`;
  await mkdir(dirname(lockPath), { recursive: true, mode: 0o700 });
  let handle;
  for (let attempt = 0; attempt < 100; attempt++) {
    try {
      handle = await open(lockPath, "wx", 0o600);
      break;
    } catch (error) {
      if (error?.code !== "EEXIST") throw error;
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
  }
  if (!handle) throw new Error("orchestrator state lock remained busy for 2.5 seconds");
  try {
    const state = await loadState(statePath);
    const result = await action(state);
    await saveState(statePath, state);
    return result;
  } finally {
    await handle.close();
    await rm(lockPath, { force: true });
  }
}

export function runningCount(state) {
  return Object.values(state.tasks).filter((task) => ["running", "dispatching", "scanning"].includes(task.state)).length;
}

export function worktreeBusy(state, task) {
  return Object.values(state.tasks).some(
    (other) =>
      other.id !== task.id &&
      ["running", "dispatching", "scanning"].includes(other.state) &&
      (other.worktree === task.worktree || other.branch === task.branch),
  );
}

export function dependenciesSatisfied(state, task) {
  return (task.dependsOn ?? []).every((id) => state.tasks[id]?.state === "done");
}

export function nextEligible(state, limit) {
  if (runningCount(state) >= limit) return undefined;
  return Object.values(state.tasks)
    .filter((task) => task.state === "queued" && dependenciesSatisfied(state, task) && !worktreeBusy(state, task))
    .sort((a, b) => a.createdAt.localeCompare(b.createdAt))[0];
}

export function transition(task, state, detail) {
  task.state = state;
  task.history.push({ state, detail, timestamp: new Date().toISOString() });
}
