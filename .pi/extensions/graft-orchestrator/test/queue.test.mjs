import assert from "node:assert/strict";
import { mkdtemp, readFile } from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { dependenciesSatisfied, nextEligible, transition, withStateLock } from "../queue.mjs";

const root = await mkdtemp(path.join(os.tmpdir(), "graft-orch-queue-"));
const statePath = path.join(root, "state.json");
const task = (id, worktree, branch, createdAt) => ({ id, worktree, branch, state: "queued", createdAt, history: [] });

await withStateLock(statePath, (state) => {
  state.tasks.one = task("one", "/tmp/worktree-one", "branch-one", "2026-01-01T00:00:00.000Z");
  state.tasks.two = task("two", "/tmp/worktree-one", "branch-two", "2026-01-01T00:00:01.000Z");
  state.tasks.three = task("three", "/tmp/worktree-three", "branch-three", "2026-01-01T00:00:02.000Z");
  state.tasks.four = { ...task("four", "/tmp/worktree-four", "branch-four", "2025-12-31T00:00:00.000Z"), dependsOn: ["one"] };
  assert.equal(dependenciesSatisfied(state, state.tasks.four), false, "unfinished predecessor blocks dependent task");
  const first = nextEligible(state, 2);
  assert.equal(first.id, "one");
  transition(first, "running", "test dispatch");
  first.state = "scanning";
  const second = nextEligible(state, 2);
  assert.equal(second.id, "three", "same worktree is locked while one runs automated scans");
  transition(second, "running", "test dispatch");
  assert.equal(nextEligible(state, 2), undefined, "global concurrency cap blocks another dispatch");
  transition(state.tasks.one, "done", "predecessor complete");
  assert.equal(dependenciesSatisfied(state, state.tasks.four), true, "terminal predecessor releases dependency gate");
});

const persisted = JSON.parse(await readFile(statePath, "utf8"));
assert.equal(persisted.tasks.one.state, "done");
assert.equal(persisted.tasks.three.state, "running");
assert.equal(persisted.tasks.one.history.length, 2);
console.log("queue lock and eligibility test passed");
