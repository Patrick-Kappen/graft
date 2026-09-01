#!/usr/bin/env node
import { randomUUID } from "node:crypto";
import { spawn } from "node:child_process";
import { mkdir, readFile, rm, appendFile, chmod } from "node:fs/promises";
import net from "node:net";
import path from "node:path";
import { StringDecoder } from "node:string_decoder";

function usage(message) {
  console.error(`graft-orchestrator runner: ${message}`);
  process.exitCode = 2;
}

function parseArgs(argv) {
  if (argv.length !== 2 || argv[0] !== "--config") return null;
  return argv[1];
}

function assertConfig(value) {
  if (!value || typeof value !== "object") throw new Error("config must be an object");
  for (const key of ["taskId", "cwd", "socketPath", "eventLog", "sessionFile"]) {
    if (typeof value[key] !== "string" || value[key].trim() === "") {
      throw new Error(`config.${key} must be a non-empty string`);
    }
  }
  if (value.command !== undefined && typeof value.command !== "string") {
    throw new Error("config.command must be a string when present");
  }
  if (value.args !== undefined && (!Array.isArray(value.args) || !value.args.every((arg) => typeof arg === "string"))) {
    throw new Error("config.args must be a string array when present");
  }
  if (value.bridgePath !== undefined && typeof value.bridgePath !== "string") {
    throw new Error("config.bridgePath must be a string when present");
  }
  if (value.tools !== undefined && (!Array.isArray(value.tools) || !value.tools.every((tool) => typeof tool === "string"))) {
    throw new Error("config.tools must be a string array when present");
  }
  if (value.model !== undefined && typeof value.model !== "string") throw new Error("config.model must be a string when present");
  if (value.thinking !== undefined && !["off", "minimal", "low", "medium", "high", "xhigh", "max"].includes(value.thinking)) {
    throw new Error("config.thinking must be a supported Pi thinking level when present");
  }
  if (value.policy !== undefined && typeof value.policy !== "string") throw new Error("config.policy must be a string when present");
  if (value.controlSocket !== undefined && typeof value.controlSocket !== "string") throw new Error("config.controlSocket must be a string when present");
  return value;
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
      try {
        onRecord(JSON.parse(line));
      } catch (error) {
        onError(new Error(`invalid JSONL record: ${error instanceof Error ? error.message : String(error)}`));
      }
    }
  });
  stream.on("end", () => {
    buffer += decoder.end();
    if (!buffer) return;
    try {
      onRecord(JSON.parse(buffer.endsWith("\r") ? buffer.slice(0, -1) : buffer));
    } catch (error) {
      onError(new Error(`invalid final JSONL record: ${error instanceof Error ? error.message : String(error)}`));
    }
  });
}

const configPath = parseArgs(process.argv.slice(2));
if (!configPath) {
  usage("expected --config <owner-only-json-file>");
  process.exit();
}

let config;
try {
  config = assertConfig(JSON.parse(await readFile(configPath, "utf8")));
} catch (error) {
  usage(error instanceof Error ? error.message : String(error));
  process.exit();
}

await mkdir(path.dirname(config.socketPath), { recursive: true, mode: 0o700 });
await mkdir(path.dirname(config.eventLog), { recursive: true, mode: 0o700 });
await mkdir(path.dirname(config.sessionFile), { recursive: true, mode: 0o700 });
await rm(config.socketPath, { force: true });

const invocation = config.command
  ? { command: config.command, args: config.args ?? [] }
  : {
      command: "pi",
      args: [
        "--mode", "rpc", "--session", config.sessionFile, "--name", config.taskId,
        ...(config.model ? ["--model", config.model] : []),
        ...(config.thinking ? ["--thinking", config.thinking] : []),
        "-ne",
        ...(config.tools?.length ? ["--tools", config.tools.join(",")] : []),
        ...(config.bridgePath ? ["--extension", config.bridgePath] : []),
      ],
    };
const child = spawn(invocation.command, invocation.args, {
  cwd: config.cwd,
  env: {
    ...process.env,
    GRAFT_ORCH_RUNNER_SOCKET: config.socketPath,
    GRAFT_ORCH_POLICY: config.policy ?? "",
    GRAFT_ORCH_WORKTREE: config.cwd,
  },
  stdio: ["pipe", "pipe", "pipe"],
  shell: false,
});

const clients = new Set();
let controlClient;
let controlConnected = false;
let controlReconnectTimer;
const controlQueue = [];
let settled = false;
let finalText = "";
let finalState = "failed";
let shutdownStarted = false;
let displayLineOpen = false;
let assistantStreamed = false;
const usageTotals = { turns: 0, input: 0, output: 0, reasoning: 0, cacheRead: 0, cacheWrite: 0, totalTokens: 0, costUsd: 0 };

function compact(value, maximum = 180) {
  const text = typeof value === "string" ? value : JSON.stringify(value ?? {});
  const clean = text.replaceAll(/\s+/g, " ").trim();
  return clean.length <= maximum ? clean : `${clean.slice(0, maximum - 1)}…`;
}

function formatTokens(value) {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(2)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return String(value ?? 0);
}

function finishDisplayLine() {
  if (displayLineOpen) process.stderr.write("\n");
  displayLineOpen = false;
}

function addUsage(value) {
  if (!value || typeof value !== "object") return;
  usageTotals.turns += 1;
  for (const key of ["input", "output", "reasoning", "cacheRead", "cacheWrite", "totalTokens"]) usageTotals[key] += Number(value[key]) || 0;
  usageTotals.costUsd += Number(value.cost?.total) || 0;
}

function renderRpcRecord(record) {
  if (record.type === "agent_start") {
    finishDisplayLine();
    process.stderr.write("\n▶ agent started\n");
  } else if (record.type === "message_update" && record.assistantMessageEvent?.type === "text_delta") {
    if (!displayLineOpen) {
      process.stderr.write("\nassistant › ");
      displayLineOpen = true;
    }
    assistantStreamed = true;
    process.stderr.write(record.assistantMessageEvent.delta ?? "");
  } else if (record.type === "tool_execution_start") {
    finishDisplayLine();
    process.stderr.write(`\n→ ${record.toolName ?? "tool"}  ${compact(record.args)}\n`);
  } else if (record.type === "tool_execution_end") {
    finishDisplayLine();
    process.stderr.write(`${record.isError ? "✗" : "✓"} ${record.toolName ?? "tool"}${record.isError ? `  ${compact(record.result)}` : ""}\n`);
  } else if (record.type === "message_end" && record.message?.role === "assistant") {
    finishDisplayLine();
    if (!assistantStreamed) {
      const text = record.message.content?.find((part) => part?.type === "text")?.text;
      if (text) process.stderr.write(`\nassistant › ${text}\n`);
    }
    assistantStreamed = false;
    addUsage(record.message.usage);
    if (record.message.usage) {
      const u = record.message.usage;
      process.stderr.write(`usage · in ${formatTokens(u.input)} · out ${formatTokens(u.output)} · cache ${formatTokens((u.cacheRead ?? 0) + (u.cacheWrite ?? 0))} · $${(u.cost?.total ?? 0).toFixed(4)}\n`);
    }
  } else if (record.type === "agent_settled") {
    finishDisplayLine();
    process.stderr.write("\n✓ agent settled\n");
  }
}

process.stderr.write(["", "Graft agent", `task      ${config.taskId}`, `route     ${config.model ?? "default"}:${config.thinking ?? "default"}`, `worktree  ${config.cwd}`, "─".repeat(72), ""].join("\n"));

function broadcast(record) {
  const line = `${JSON.stringify(record)}\n`;
  for (const client of clients) client.write(line);
}

function connectControl() {
  if (!config.controlSocket || controlConnected || (controlClient && !controlClient.destroyed)) return;
  const socket = net.createConnection(config.controlSocket);
  controlClient = socket;
  socket.on("connect", () => {
    if (controlClient !== socket) return socket.end();
    controlConnected = true;
    while (controlQueue.length) socket.write(controlQueue.shift());
  });
  socket.on("data", () => {});
  socket.on("error", (error) => console.error(`daemon stream unavailable: ${error.message}`));
  socket.on("close", () => {
    if (controlClient !== socket) return;
    controlConnected = false;
    controlClient = undefined;
    if (!settled) controlReconnectTimer = setTimeout(connectControl, 500);
  });
}

function sendControl(event) {
  if (!config.controlSocket) return;
  const line = `${JSON.stringify({ id: randomUUID(), type: "runner_event", taskId: config.taskId, event })}\n`;
  if (controlConnected && controlClient && !controlClient.destroyed) controlClient.write(line);
  else {
    controlQueue.push(line);
    if (controlQueue.length > 256) controlQueue.shift();
    connectControl();
  }
}

connectControl();

async function persistAndExit(state, detail) {
  if (shutdownStarted) return;
  shutdownStarted = true;
  const event = {
    eventId: randomUUID(),
    type: "orchestrator_agent_settled",
    taskId: config.taskId,
    state,
    finalText,
    detail,
    timestamp: new Date().toISOString(),
    sessionFile: config.sessionFile,
    model: config.model,
    thinking: config.thinking,
    usage: { ...usageTotals },
  };
  await appendFile(config.eventLog, `${JSON.stringify(event)}\n`, { encoding: "utf8", mode: 0o600 });
  broadcast(event);
  sendControl(event);
  server.close();
  await rm(config.socketPath, { force: true });
  for (const client of clients) client.end();
  if (controlReconnectTimer) clearTimeout(controlReconnectTimer);
  controlClient?.end();
  child.kill("SIGTERM");
  setTimeout(() => process.exit(state === "done" ? 0 : 1), 25).unref();
}

function fail(error) {
  console.error(error.message);
  void persistAndExit("failed", error.message);
}

function sendToChild(record) {
  if (shutdownStarted) throw new Error("runner is settled");
  child.stdin.write(`${JSON.stringify(record)}\n`);
}

attachJsonl(
  child.stdout,
  (record) => {
    // Keep protocol stdout private; render a concise human view in the tmux pane.
    renderRpcRecord(record);
    broadcast({ type: "pi_rpc_event", event: record });
    sendControl(record);
    if (record.type === "message_end" && record.message?.role === "assistant") {
      const text = record.message.content?.find((part) => part?.type === "text")?.text;
      if (typeof text === "string") finalText = text;
    }
    if (record.type === "agent_settled") {
      settled = true;
      finalState = "done";
      void persistAndExit("done", "agent_settled");
    }
  },
  fail,
);
child.stderr.on("data", (chunk) => process.stderr.write(chunk));
child.on("error", fail);
child.on("exit", (code, signal) => {
  if (!shutdownStarted && !settled) {
    fail(new Error(`Pi RPC process exited before agent_settled (code=${code}, signal=${signal})`));
  }
});

const server = net.createServer((client) => {
  clients.add(client);
  client.write(`${JSON.stringify({ type: "runner_ready", taskId: config.taskId })}\n`);
  attachJsonl(
    client,
    (record) => {
      if (record?.type === "orchestrator_input_request" && typeof record.title === "string" && typeof record.question === "string") {
        const event = {
          eventId: randomUUID(),
          type: "orchestrator_input_requested",
          taskId: config.taskId,
          title: record.title,
          question: record.question,
          timestamp: new Date().toISOString(),
        };
        void appendFile(config.eventLog, `${JSON.stringify(event)}\n`, { encoding: "utf8", mode: 0o600 })
          .then(() => {
            broadcast(event);
            client.write(`${JSON.stringify({ type: "runner_ack", request: "orchestrator_input_request" })}\n`);
          })
          .catch(fail);
        return;
      }
      if (!record || record.type !== "prompt" || typeof record.message !== "string" || record.message.trim() === "") {
        client.write(`${JSON.stringify({ type: "runner_error", error: "expected non-empty prompt or decision request" })}\n`);
        return;
      }
      try {
        sendToChild({ id: record.id ?? `prompt-${Date.now()}`, type: "prompt", message: record.message });
      } catch (error) {
        client.write(`${JSON.stringify({ type: "runner_error", error: error instanceof Error ? error.message : String(error) })}\n`);
      }
    },
    fail,
  );
  client.on("close", () => clients.delete(client));
  client.on("error", () => clients.delete(client));
});
server.on("error", fail);
server.listen(config.socketPath, async () => {
  await chmod(config.socketPath, 0o600);
  console.error(`runner ready: ${config.taskId}`);
});

for (const signal of ["SIGTERM", "SIGHUP"]) {
  process.on(signal, () => {
    if (!shutdownStarted) void persistAndExit("failed", `runner terminated by ${signal}`);
  });
}
