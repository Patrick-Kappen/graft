import assert from "node:assert/strict";
import { access, mkdtemp, mkdir, readFile, writeFile } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { spawn } from "node:child_process";

const extensionRoot = path.resolve(import.meta.dirname, "..");
const temp = await mkdtemp(path.join(os.tmpdir(), "graft-orch-runner-"));
const socketPath = path.join(temp, "agent.sock");
const eventLog = path.join(temp, "events.jsonl");
const configPath = path.join(temp, "runner.json");
const sessionFile = path.join(temp, "session.jsonl");
const mock = path.join(extensionRoot, "test", "mock-pi.mjs");
await mkdir(temp, { recursive: true, mode: 0o700 });
await writeFile(
  configPath,
  `${JSON.stringify({
    taskId: "runner-smoke",
    cwd: temp,
    socketPath,
    eventLog,
    sessionFile,
    thinking: "high",
    command: process.execPath,
    args: [mock],
  })}\n`,
  { mode: 0o600 },
);

const runner = spawn(process.execPath, [path.join(extensionRoot, "runner.mjs"), "--config", configPath], {
  stdio: ["ignore", "pipe", "pipe"],
});
let stderr = "";
runner.stderr.on("data", (chunk) => (stderr += chunk.toString("utf8")));

for (let attempt = 0; attempt < 50; attempt++) {
  try {
    await new Promise((resolve, reject) => {
      const probe = net.createConnection(socketPath);
      probe.once("connect", () => {
        probe.end();
        resolve(undefined);
      });
      probe.once("error", reject);
    });
    break;
  } catch {
    await new Promise((resolve) => setTimeout(resolve, 20));
  }
  if (attempt === 49) throw new Error(`runner socket did not become ready: ${stderr}`);
}

const received = await new Promise((resolve, reject) => {
  const client = net.createConnection(socketPath);
  let buffer = "";
  const timer = setTimeout(() => reject(new Error("timed out waiting for runner completion")), 3000);
  client.on("connect", () =>
    client.write(
      `${JSON.stringify({ type: "orchestrator_input_request", title: "Need review", question: "Proceed?" })}\n` +
        `${JSON.stringify({ type: "prompt", id: "smoke", message: "hello" })}\n`,
    ),
  );
  client.on("data", (chunk) => {
    buffer += chunk.toString("utf8");
    if (!buffer.includes('"orchestrator_agent_settled"')) return;
    clearTimeout(timer);
    client.end();
    resolve(buffer);
  });
  client.on("error", reject);
});
const exitCode = await new Promise((resolve) => runner.on("exit", (code) => resolve(code)));
const events = (await readFile(eventLog, "utf8")).trim().split("\n").map(JSON.parse);

assert.equal(exitCode, 0, stderr);
assert.equal(events.filter((event) => event.type === "orchestrator_agent_settled").length, 1);
const settled = events.find((event) => event.type === "orchestrator_agent_settled");
const decision = events.find((event) => event.type === "orchestrator_input_requested");
assert.deepEqual(settled.state, "done");
assert.equal(settled.taskId, "runner-smoke");
assert.equal(settled.finalText, "mock completed: hello");
assert.equal(settled.usage.turns, 1);
assert.equal(settled.usage.totalTokens, 630);
assert.equal(settled.usage.cacheRead, 480);
assert.equal(settled.usage.costUsd, 0.0035);
assert.doesNotMatch(stderr, /\{"type":"message_end"/, "runner pane must not mirror raw RPC JSON");
assert.match(stderr, /usage · in 120 · out 30 · cache 480 · \$0\.0035/);
assert.equal(decision.title, "Need review");
assert.equal(decision.question, "Proceed?");
assert.match(received, /orchestrator_agent_settled/);
await assert.rejects(access(socketPath), /ENOENT/);
const invalidConfigPath = path.join(temp, "invalid-thinking.json");
await writeFile(
  invalidConfigPath,
  `${JSON.stringify({ taskId: "invalid-thinking", cwd: temp, socketPath, eventLog, sessionFile, thinking: "turbo", command: process.execPath, args: [mock] })}\n`,
  { mode: 0o600 },
);
const invalid = spawn(process.execPath, [path.join(extensionRoot, "runner.mjs"), "--config", invalidConfigPath], { stdio: ["ignore", "ignore", "pipe"] });
let invalidStderr = "";
invalid.stderr.on("data", (chunk) => (invalidStderr += chunk.toString("utf8")));
const invalidExit = await new Promise((resolve) => invalid.on("exit", resolve));
assert.equal(invalidExit, 2);
assert.match(invalidStderr, /supported Pi thinking level/);
console.log("runner smoke test passed");
