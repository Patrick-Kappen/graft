#!/usr/bin/env node
import process from "node:process";
import { subscribe } from "./client.mjs";

const args = process.argv.slice(2);
const value = (name) => {
  const index = args.indexOf(name);
  return index >= 0 ? args[index + 1] : undefined;
};
const socketPath = value("--socket");
const taskId = value("--task");
if (!socketPath || !taskId) {
  console.error("usage: viewer.mjs --socket <path> --task <id>");
  process.exit(2);
}

let snapshot = { tasks: [], activities: {}, usageSummary: {} };
let reconnectMessage = "connecting";

function tokens(value) {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(2)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return String(value ?? 0);
}

function wrap(text, width) {
  const words = String(text ?? "").replaceAll(/\s+/g, " ").trim().split(" ").filter(Boolean);
  const lines = [];
  let line = "";
  for (const word of words) {
    if (!line || line.length + word.length + 1 <= width) line += `${line ? " " : ""}${word}`;
    else { lines.push(line); line = word; }
  }
  if (line) lines.push(line);
  return lines;
}

function render() {
  const task = snapshot.tasks.find((item) => item.id === taskId);
  const activity = snapshot.activities?.[taskId] ?? {};
  const width = Math.max(50, process.stdout.columns || 100);
  const rule = "─".repeat(Math.min(width, 100));
  const lines = [
    "Graft agent view",
    rule,
    `task       ${taskId}`,
    `state      ${task?.state ?? reconnectMessage}`,
    `phase      ${task?.phase ?? task?.profile ?? "unknown"}`,
    `route      ${task?.model ?? "unknown"}:${task?.thinkingLevel ?? task?.thinking ?? "default"}`,
    `worktree   ${task?.worktree ?? "unknown"}`,
    rule,
    `activity   ${activity.tool ? `${activity.phase ?? "tool"}: ${activity.tool}` : activity.phase ?? "waiting"}`,
  ];
  if (activity.detail) lines.push(...wrap(`detail     ${activity.detail}`, width));
  if (activity.text) lines.push("", "latest response", ...wrap(activity.text, width));
  const usage = task?.usage;
  if (usage) {
    const prompt = (usage.input ?? 0) + (usage.cacheRead ?? 0) + (usage.cacheWrite ?? 0);
    const cache = prompt ? ((usage.cacheRead ?? 0) / prompt) * 100 : 0;
    lines.push("", rule, `usage      ${tokens(usage.totalTokens)} tokens · in ${tokens(usage.input)} · out ${tokens(usage.output)} · cache ${tokens((usage.cacheRead ?? 0) + (usage.cacheWrite ?? 0))} (${cache.toFixed(1)}%) · $${(usage.costUsd ?? 0).toFixed(4)}`);
  }
  if (task?.result?.finalText && !["running", "dispatching", "scanning"].includes(task.state)) lines.push("", "result", ...wrap(task.result.finalText, width));
  lines.push("", "q close · return to the orchestrator with your tmux previous-window key");
  process.stdout.write(`\x1b[2J\x1b[H${lines.join("\n")}\n`);
}

const stream = subscribe(
  socketPath,
  (record) => {
    if (record.type === "response" && record.success && record.data?.tasks) snapshot = record.data;
    else if (record.tasks) snapshot = record;
    else if (record.type === "activity") snapshot.activities = { ...snapshot.activities, [record.activity.taskId]: record.activity };
    reconnectMessage = "connected";
    render();
  },
  (error) => { reconnectMessage = `reconnecting: ${error.message}`; render(); },
);

if (process.stdin.isTTY) process.stdin.setRawMode(true);
process.stdin.resume();
process.stdin.on("data", (chunk) => {
  const key = chunk.toString("utf8");
  if (key === "q" || key === "Q" || key === "\u0003" || key === "\u001b") {
    stream.end();
    if (process.stdin.isTTY) process.stdin.setRawMode(false);
    process.exit(0);
  }
});
process.on("SIGTERM", () => { stream.end(); process.exit(0); });
render();
