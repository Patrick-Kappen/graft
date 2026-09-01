import { StringDecoder } from "node:string_decoder";

const decoder = new StringDecoder("utf8");
let buffer = "";

function emit(value) {
  process.stdout.write(`${JSON.stringify(value)}\n`);
}

process.stdin.on("data", (chunk) => {
  buffer += decoder.write(chunk);
  while (true) {
    const index = buffer.indexOf("\n");
    if (index < 0) break;
    const line = buffer.slice(0, index);
    buffer = buffer.slice(index + 1);
    if (!line) continue;
    const command = JSON.parse(line);
    if (command.type === "prompt") {
      emit({ type: "response", id: command.id, command: "prompt", success: true });
      emit({ type: "agent_start" });
      emit({
        type: "message_end",
        message: {
          role: "assistant",
          content: [{ type: "text", text: `mock completed: ${command.message}` }],
          usage: { input: 120, output: 30, reasoning: 10, cacheRead: 480, cacheWrite: 0, totalTokens: 630, cost: { input: 0.001, output: 0.002, cacheRead: 0.0005, cacheWrite: 0, total: 0.0035 } },
        },
      });
      emit({ type: "agent_settled" });
    }
  }
});
