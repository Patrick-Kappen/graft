import { randomUUID } from "node:crypto";
import net from "node:net";
import { StringDecoder } from "node:string_decoder";

export function attachJsonl(stream, onRecord, onError) {
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
      catch (error) { onError(error); }
    }
  });
}

export function request(socketPath, command, timeoutMs = 15000) {
  return new Promise((resolve, reject) => {
    const id = command.id ?? randomUUID();
    const socket = net.createConnection(socketPath);
    const timer = setTimeout(() => { socket.destroy(); reject(new Error(`daemon request ${command.type} timed out`)); }, timeoutMs);
    attachJsonl(socket, (record) => {
      if (record.type !== "response" || record.id !== id) return;
      clearTimeout(timer);
      socket.end();
      if (record.success) resolve(record.data);
      else reject(new Error(record.error ?? "daemon request failed"));
    }, reject);
    socket.on("connect", () => socket.write(`${JSON.stringify({ ...command, id })}\n`));
    socket.on("error", (error) => { clearTimeout(timer); reject(error); });
  });
}

export function subscribe(socketPath, onRecord, onError, reconnectMs = 500) {
  let socket;
  let reconnectTimer;
  let stopped = false;

  const connect = () => {
    if (stopped) return;
    const current = net.createConnection(socketPath);
    socket = current;
    const id = randomUUID();
    attachJsonl(current, onRecord, onError);
    current.on("connect", () => current.write(`${JSON.stringify({ id, type: "subscribe" })}\n`));
    current.on("error", (error) => {
      if (!stopped && !["ECONNREFUSED", "ECONNRESET", "ENOENT", "EPIPE"].includes(error.code)) onError(error);
    });
    current.on("close", () => {
      if (socket === current) socket = undefined;
      if (!stopped && !reconnectTimer) {
        reconnectTimer = setTimeout(() => {
          reconnectTimer = undefined;
          connect();
        }, reconnectMs);
        reconnectTimer.unref?.();
      }
    });
  };

  connect();
  return {
    end() {
      stopped = true;
      if (reconnectTimer) clearTimeout(reconnectTimer);
      reconnectTimer = undefined;
      socket?.end();
      socket?.destroy();
      socket = undefined;
    },
  };
}
