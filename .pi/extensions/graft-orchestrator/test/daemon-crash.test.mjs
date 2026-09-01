import assert from "node:assert/strict";
import { access, chmod, mkdtemp, mkdir, writeFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import os from "node:os";
import path from "node:path";
import { request } from "../client.mjs";

const extensionRoot = path.resolve(import.meta.dirname, "..");
const repo = path.resolve(extensionRoot, "../../../..");
const temp = await mkdtemp(path.join(os.tmpdir(), "graft-orch-daemon-crash-"));
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
const taskId = `missing-runner-${process.pid}`;
const staleTaskDir = path.join(runtimeDir, "tasks", taskId);
await mkdir(staleTaskDir, { recursive: true });
await writeFile(path.join(staleTaskDir, "agent.sock"), "stale socket inode fixture");
await writeFile(path.join(stateDir, "state.json"), `${JSON.stringify({
  version: 1,
  tasks: {
    [taskId]: {
      id: taskId,
      ticket: 364,
      state: "running",
      runningSince: "2026-01-01T00:00:00.000Z",
      createdAt: "2026-01-01T00:00:00.000Z",
      history: [],
      mailbox: [],
    },
  },
  admissions: {}, decisions: {},
})}\n`);
const daemon = spawn(process.execPath, [path.join(extensionRoot, "daemon.mjs"), "--repo", repo], {
  env: {
    ...process.env,
    PATH: `${fakeBin}:${process.env.PATH}`,
    XDG_RUNTIME_DIR: runtime,
    XDG_STATE_HOME: stateRoot,
    GRAFT_ORCH_TMUX_SESSION: `missing-orch-${process.pid}`,
    GRAFT_ORCH_RUNNER_GRACE_MS: "1000",
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
let failed = false;
for (let attempt = 0; attempt < 100; attempt++) {
  const snapshot = await request(socket, { type: "snapshot" });
  const task = snapshot.tasks.find((item) => item.id === taskId);
  if (task?.state === "failed") {
    assert.match(task.history.at(-1).detail, /runner disappeared/);
    await assert.rejects(access(path.join(staleTaskDir, "agent.sock")), /ENOENT/);
    failed = true;
    break;
  }
  await new Promise((resolve) => setTimeout(resolve, 50));
}
const exit = daemon.exitCode === null ? new Promise((resolve) => daemon.once("exit", resolve)) : undefined;
daemon.kill("SIGTERM");
if (exit) await exit;
assert.ok(failed, `missing runner was not reconciled: ${stderr}`);
console.log("daemon crash reconciliation test passed");
