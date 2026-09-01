import assert from "node:assert/strict";
import { mkdtemp } from "node:fs/promises";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { subscribe } from "../client.mjs";

const temp = await mkdtemp(path.join(os.tmpdir(), "graft-orch-client-"));
const socketPath = path.join(temp, "control.sock");
const peers = new Set();

async function startServer(generation) {
  const server = net.createServer((socket) => {
    peers.add(socket);
    socket.on("close", () => peers.delete(socket));
    socket.once("data", () => {
      socket.write(`${JSON.stringify({ type: "response", success: true, data: { tasks: [], generation } })}\n`);
    });
  });
  await new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(socketPath, resolve);
  });
  return server;
}

let server = await startServer(1);
const generations = [];
let wake;
const waitForRecord = () => new Promise((resolve) => { wake = resolve; });
let nextRecord = waitForRecord();
const subscription = subscribe(
  socketPath,
  (record) => {
    if (record.data?.generation) generations.push(record.data.generation);
    wake?.();
  },
  (error) => { throw error; },
  25,
);
await Promise.race([nextRecord, new Promise((_, reject) => setTimeout(() => reject(new Error("first subscription timed out")), 1000))]);
for (const peer of peers) peer.destroy();
await new Promise((resolve) => server.close(resolve));

nextRecord = waitForRecord();
await new Promise((resolve) => setTimeout(resolve, 75));
server = await startServer(2);
await Promise.race([nextRecord, new Promise((_, reject) => setTimeout(() => reject(new Error("reconnection timed out")), 1500))]);

subscription.end();
for (const peer of peers) peer.destroy();
await new Promise((resolve) => server.close(resolve));
assert.deepEqual(generations, [1, 2]);
console.log("client subscription reconnect test passed");
