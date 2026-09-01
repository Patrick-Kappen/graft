import net from "node:net";
import path from "node:path";
import { isToolCallEventType, type ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";

function sendRequest(socketPath: string, payload: Record<string, string>): Promise<void> {
  return new Promise((resolve, reject) => {
    const socket = net.createConnection(socketPath);
    let buffer = "";
    const timer = setTimeout(() => {
      socket.destroy();
      reject(new Error("orchestrator runner did not acknowledge decision within 5 seconds"));
    }, 5000);
    socket.on("connect", () => socket.write(`${JSON.stringify({ type: "orchestrator_input_request", ...payload })}\n`));
    socket.on("data", (chunk) => {
      buffer += chunk.toString("utf8");
      if (!buffer.includes("\n")) return;
      clearTimeout(timer);
      socket.end();
      resolve();
    });
    socket.on("error", (error) => {
      clearTimeout(timer);
      reject(error);
    });
  });
}

export default function (pi: ExtensionAPI) {
  pi.on("tool_call", (event) => {
    const policy = process.env.GRAFT_ORCH_POLICY;
    if (["design-read-only", "review-changes-read-only"].includes(policy ?? "") && (isToolCallEventType("bash", event) || event.toolName === "write" || event.toolName === "edit")) {
      return {
        block: true,
        reason: policy === "review-changes-read-only"
          ? "Fresh-review policy forbids shell execution and filesystem mutation. Review only the generated change bundle and changed files with read-only tools."
          : "Design-scout policy forbids shell execution and filesystem mutation. Use read, grep, find, or ls; ask the orchestrator for runtime evidence.",
      };
    }
    if (!["implementation-worktree", "verification-worktree"].includes(policy ?? "")) return;
    if (isToolCallEventType("bash", event)) {
      const command = event.input.command;
      const hostRuntime = /(?:^|[\n;&|]\s*)(?:sudo\s+)?(?:(?:env|timeout)\s+\S+\s+)*(?:systemctl|systemd-run|loginctl|podman|docker)\b/m;
      const gitMutation = /(?:^|[\n;&|]\s*)git(?:\s+-C\s+\S+)?\s+(?:commit|push|tag|reset|clean|checkout|switch|worktree|branch\s+(?:-[dDmM]|--delete|--move))\b/m;
      const githubMutation = /(?:^|[\n;&|]\s*)gh\s+(?:issue\s+(?:create|edit|close|comment)|pr\s+(?:create|merge|close|review)|workflow\s+run|release\s+|api\b.*(?:-X|--method))/m;
      if (hostRuntime.test(command) || gitMutation.test(command) || githubMutation.test(command)) {
        return {
          block: true,
          reason: "Orchestrator policy blocks host runtime control, Git history/ref mutation, and GitHub mutation. Use repository-local builds/Nix VMs or request an external-action gate.",
        };
      }
    }
    if (event.toolName === "write" || event.toolName === "edit") {
      if (policy === "verification-worktree") {
        return { block: true, reason: "Verification policy is read-only for tracked files." };
      }
      const inputPath = String((event.input as { path?: string }).path ?? "");
      const worktree = process.env.GRAFT_ORCH_WORKTREE;
      if (!worktree || !inputPath) return { block: true, reason: "Worktree-scoped write has no validated path." };
      const absolute = path.resolve(worktree, inputPath);
      const root = path.resolve(worktree);
      if (absolute !== root && !absolute.startsWith(`${root}${path.sep}`)) {
        return { block: true, reason: `Writes are restricted to ${root}.` };
      }
    }
  });

  pi.registerTool({
    name: "request_orchestrator_input",
    label: "Request Orchestrator Input",
    description: "Stop and send a bounded question/evidence item to the orchestrator. The orchestrator may answer it or escalate it to the maintainer.",
    parameters: Type.Object({
      title: Type.String({ minLength: 1, maxLength: 160 }),
      question: Type.String({ minLength: 1, maxLength: 4000 }),
    }),
    async execute(_id, params) {
      const socketPath = process.env.GRAFT_ORCH_RUNNER_SOCKET;
      if (!socketPath) throw new Error("orchestrator runner socket is unavailable");
      await sendRequest(socketPath, { title: params.title, question: params.question });
      return {
        content: [{ type: "text", text: "Orchestrator input requested. Do not guess the answer; settle and await a follow-up prompt." }],
        details: {},
      };
    },
  });
}
