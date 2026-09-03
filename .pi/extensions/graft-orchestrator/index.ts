import { join, resolve } from "node:path";
import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { StringEnum } from "@earendil-works/pi-ai";
import { Type } from "typebox";
import { truncateToWidth, visibleWidth } from "@earendil-works/pi-tui";
import { ExplanationModal, maintainerItems, OrchDashboard, OrchDetailModal } from "./dashboard.ts";
import { request, subscribe } from "./client.mjs";

const runtimeRoot = join(process.env.XDG_RUNTIME_DIR ?? "/tmp", "pi-orch");
const controlSocket = join(runtimeRoot, "control.sock");
const tmuxSession = "graft-v04-pi";
let snapshot: any = { tasks: [], admissions: [], decisions: [], activities: {} };

function shellQuote(value: string): string {
  return `'${value.replaceAll("'", "'\"'\"'")}'`;
}

const SPINNER = ["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"];

function reviewIsDue(task: any) {
  if (!task?.reviewNotBefore) return true;
  const due = Date.parse(task.reviewNotBefore);
  return !Number.isFinite(due) || due <= Date.now();
}

function compactAge(timestamp: string | undefined) {
  if (!timestamp) return "unknown";
  const elapsedSeconds = Math.max(0, Math.floor((Date.now() - Date.parse(timestamp)) / 1000));
  if (!Number.isFinite(elapsedSeconds)) return "unknown";
  if (elapsedSeconds < 60) return `${elapsedSeconds}s`;
  if (elapsedSeconds < 3600) return `${Math.floor(elapsedSeconds / 60)}m`;
  if (elapsedSeconds < 86400) return `${Math.floor(elapsedSeconds / 3600)}h ${Math.floor((elapsedSeconds % 3600) / 60)}m`;
  return `${Math.floor(elapsedSeconds / 86400)}d ${Math.floor((elapsedSeconds % 86400) / 3600)}h`;
}

function latestRunStarted(task: any) {
  return [...(task.history ?? [])]
    .reverse()
    .find((entry: any) => ["running", "dispatching"].includes(entry.state))?.timestamp;
}

function counts() {
  const tasks = snapshot.tasks ?? [];
  const running = tasks.filter((task: any) => ["running", "dispatching", "scanning"].includes(task.state));
  const legacyReviews = tasks.filter((task: any) => task.state === "awaiting_review");
  const freshReviewTasks = tasks.filter((task: any) => task.profile === "change-review" && ["queued", "dispatching", "running"].includes(task.state));
  const deferredReviews = tasks.filter((task: any) => ["review_deferred", "preparing_review"].includes(task.state));
  const reviews = [...legacyReviews, ...freshReviewTasks, ...deferredReviews];
  return {
    running,
    scanning: running.filter((task: any) => task.state === "scanning"),
    reviews,
    reviewQueue: [...legacyReviews.filter((task: any) => !task.reviewClaimedAt), ...freshReviewTasks.filter((task: any) => task.state === "queued"), ...deferredReviews],
    reviewing: [...legacyReviews.filter((task: any) => task.reviewClaimedAt), ...freshReviewTasks.filter((task: any) => ["dispatching", "running"].includes(task.state))],
    queued: tasks.filter((task: any) => task.state === "queued"),
    eligibleQueued: tasks.filter((task: any) => task.state === "queued" && (task.dependsOn ?? []).every((id: string) => tasks.some((dependency: any) => dependency.id === id && dependency.state === "done"))),
    decisions: (snapshot.decisions ?? []).filter((item: any) => item.state === "pending"),
    admissions: (snapshot.admissions ?? []).filter((item: any) => item.state === "pending"),
  };
}


export default function (pi: ExtensionAPI) {
  let subscription: any;
  let dashboard: OrchDashboard | undefined;
  let reviewInFlight = false;
  let activeContext: any;
  let spinnerFrame = 0;
  let spinnerTimer: ReturnType<typeof setInterval> | undefined;
  let snapshotRefresh: Promise<void> | undefined;

  const orchestratorState = (c: ReturnType<typeof counts>, actions: any[]) => {
    if (reviewInFlight) return "claiming";
    if (c.reviewing.length) return "reviewing";
    if (actions.length) return "waiting for user";
    if (c.running.some((task: any) => task.state === "dispatching") || c.eligibleQueued.length) return "dispatching";
    if (c.scanning.length) return "scanning";
    if (c.running.length) return "monitoring";
    return "idle";
  };

  const renderStatusBar = (ctx: any) => {
    const c = counts();
    const actions = maintainerItems(snapshot);
    const state = orchestratorState(c, actions);
    const spin = SPINNER[spinnerFrame % SPINNER.length];
    ctx.ui.setWidget("graft-orchestrator", (_tui: any, theme: any) => ({
      invalidate() {},
      render(width: number) {
        const activeValue = c.running.length
          ? theme.fg("accent", theme.bold(`${c.running.length} ${spin}`))
          : theme.fg("muted", "0 ○");
        const userValue = actions.length
          ? theme.fg("warning", theme.bold(String(actions.length)))
          : theme.fg("success", "0");
        const reviewValue = c.reviewQueue.length
          ? theme.fg("warning", theme.bold(String(c.reviewQueue.length)))
          : theme.fg("muted", "0");
        const stateColor = state === "idle" ? "muted" : state === "waiting for user" ? "warning" : state === "reviewing" || state === "claiming" ? "accent" : "success";
        const stateSpinner = ["reviewing", "claiming", "dispatching", "scanning"].includes(state) ? ` ${spin}` : "";
        const parts = [
          `${theme.fg("muted", theme.bold("User:"))} ${userValue}`,
          `${theme.fg("muted", theme.bold("Active:"))} ${activeValue}`,
          `${theme.fg("muted", theme.bold("Review Queue:"))} ${reviewValue}`,
          `${theme.fg("dim", "→")} ${theme.fg("accent", theme.bold("Orchestrator:"))} ${theme.fg(stateColor, theme.bold(`${state}${stateSpinner}`))}`,
        ];
        const separators = parts.length - 1;
        const available = Math.max(parts.length, width - separators);
        const baseWidth = Math.floor(available / parts.length);
        let remainder = available - baseWidth * parts.length;
        const centered = parts.map((part, index) => {
          const columnWidth = baseWidth + (remainder-- > 0 ? 1 : 0);
          const clipped = truncateToWidth(part, Math.max(1, columnWidth));
          const padding = Math.max(0, columnWidth - visibleWidth(clipped));
          const left = Math.floor(padding / 2);
          return `${" ".repeat(left)}${clipped}${" ".repeat(padding - left)}`;
        });
        const progress = c.running.slice(0, 3).map((task: any) => {
          const activity = snapshot.activities?.[task.id];
          const identity = task.ticket ? `#${task.ticket}` : task.id;
          const phase = task.phase ?? "task";
          const lifecycle = String(task.state ?? "unknown").replaceAll("_", " ");
          const reviewRound = task.reviewAttempts > 0 && ["queued", "dispatching", "running", "scanning"].includes(task.state)
            ? ` · review fix ${task.reviewAttempts}`
            : "";
          const step = task.state === "scanning"
            ? "automatic scans"
            : activity?.tool
              ? `${activity.phase ?? "tool"}: ${activity.tool}`
              : activity?.phase ?? lifecycle;
          const runningFor = compactAge(latestRunStarted(task));
          const updatedAgo = compactAge(activity?.updatedAt ?? task.history?.at(-1)?.timestamp);
          const text = `↳ ${identity} · ${phase} / ${lifecycle}${reviewRound} · ${step} · active ${runningFor} · update ${updatedAgo} ago`;
          return truncateToWidth(theme.fg("muted", text), Math.max(1, width));
        });
        if (c.running.length > progress.length) {
          progress.push(truncateToWidth(theme.fg("dim", `  +${c.running.length - progress.length} more active agents`), Math.max(1, width)));
        }
        return [truncateToWidth(centered.join(theme.fg("dim", "│")), Math.max(1, width)), ...progress];
      },
    }));
    ctx.ui.setStatus("graft-orchestrator", undefined);
  };

  const refreshWidget = (ctx: any) => {
    renderStatusBar(ctx);
    dashboard?.refresh();
  };

  const refreshSnapshot = (ctx: any) => {
    if (snapshotRefresh) return snapshotRefresh;
    snapshotRefresh = request(controlSocket, { type: "snapshot" })
      .then((next) => {
        snapshot = next;
        refreshWidget(ctx);
        void maybeClaimReview();
      })
      .catch(() => {})
      .finally(() => { snapshotRefresh = undefined; });
    return snapshotRefresh;
  };

  const ensureDaemon = async (ctx: any) => {
    try {
      await request(controlSocket, { type: "ping" }, 1000);
      return;
    } catch {}
    const hasTmux = await pi.exec("tmux", ["has-session", "-t", tmuxSession], { timeout: 3000 });
    if (hasTmux.code !== 0) {
      const created = await pi.exec("tmux", ["new-session", "-d", "-s", tmuxSession, "-n", "control"], { timeout: 3000 });
      if (created.code !== 0) throw new Error(created.stderr || "cannot create orchestrator tmux session");
    }
    const daemonWindow = `${tmuxSession}:orch-daemon`;
    const stale = await pi.exec("tmux", ["has-session", "-t", daemonWindow], { timeout: 3000 });
    if (stale.code === 0) await pi.exec("tmux", ["kill-window", "-t", daemonWindow], { timeout: 3000 });
    const daemonPath = resolve(ctx.cwd, ".pi/extensions/graft-orchestrator/daemon.mjs");
    const command = `exec ${shellQuote(process.execPath)} ${shellQuote(daemonPath)} --repo ${shellQuote(ctx.cwd)}`;
    const launched = await pi.exec("tmux", ["new-window", "-d", "-t", tmuxSession, "-n", "orch-daemon", command], { timeout: 5000 });
    if (launched.code !== 0) throw new Error(launched.stderr || "cannot start orchestrator daemon");
    for (let attempt = 0; attempt < 100; attempt++) {
      try { await request(controlSocket, { type: "ping" }, 500); return; }
      catch { await new Promise((resolvePromise) => setTimeout(resolvePromise, 50)); }
    }
    throw new Error("orchestrator daemon did not become ready");
  };

  const maybeClaimReview = async () => {
    if (process.env.GRAFT_ORCH_AUTO_REVIEW === "0" || reviewInFlight || !activeContext?.isIdle()) return;
    const available = snapshot.tasks.some((task: any) => task.state === "awaiting_review" && !task.reviewClaimedAt && reviewIsDue(task));
    if (!available) return;
    reviewInFlight = true;
    try {
      const task: any = await request(controlSocket, { type: "claim_review" });
      if (!task) return;
      const localTask = (snapshot.tasks ?? []).find((item: any) => item.id === task.id);
      if (localTask) localTask.reviewClaimedAt = task.reviewClaimedAt ?? new Date().toISOString();
      if (activeContext) renderStatusBar(activeContext);
      const result = task.result?.finalText ?? "(agent returned no final text)";
      pi.sendMessage(
        {
          customType: "graft-orchestrator-review",
          content: [
            `Orchestrator review required for task ${task.id}, GitHub #${task.ticket}.`,
            `Worktree: ${task.worktree}`,
            `Model route: ${task.model ?? "default"}:${task.thinkingLevel ?? task.thinking ?? "default"}`,
            `Baseline: ${task.baseline ?? "unknown"}`,
            task.inputRequests?.length ? `Agent questions:\n${task.inputRequests.map((item: any) => `${item.title}: ${item.question}`).join("\n")}` : "Agent questions: none",
            task.preflightRun?.state === "green"
              ? `Automatic preflight: GREEN (${task.preflightRun.scans?.length ?? 0} scans passed; raw lint output remained private).`
              : task.preflightRun?.state === "red"
                ? `Automatic preflight: RED after ${task.preflightRun.attempt} private repair attempts; raw lint output remains in private scan logs.`
                : "Automatic preflight: not configured.",
            "Agent result:",
            result,
            "Review the evidence and use orch_review_action exactly once. Do not treat agent_settled as acceptance.",
          ].join("\n\n"),
          display: true,
          details: { taskId: task.id, ticket: task.ticket },
        },
        { triggerTurn: true, deliverAs: "followUp" },
      );
    } catch (error) {
      activeContext?.ui.notify(`Orchestrator review claim failed: ${error instanceof Error ? error.message : String(error)}`, "error");
    } finally {
      reviewInFlight = false;
      if (activeContext) renderStatusBar(activeContext);
    }
  };

  pi.on("session_start", async (_event, ctx) => {
    activeContext = ctx;
    try {
      await ensureDaemon(ctx);
      snapshot = await request(controlSocket, { type: "snapshot" });
      refreshWidget(ctx);
      ctx.ui.setFooter((_tui: any, theme: any) => ({
        invalidate() {},
        render(width: number) {
          const label = truncateToWidth(
            theme.fg("accent", theme.bold("Orchestrator agent · Sol / medium")),
            Math.max(1, width),
          );
          const padding = Math.max(0, width - visibleWidth(label));
          return [truncateToWidth(`${" ".repeat(Math.floor(padding / 2))}${label}`, Math.max(1, width))];
        },
      }));
      spinnerTimer = setInterval(() => {
        const c = counts();
        const state = orchestratorState(c, maintainerItems(snapshot));
        if (!c.running.length && !["claiming", "reviewing", "dispatching"].includes(state)) return;
        spinnerFrame = (spinnerFrame + 1) % SPINNER.length;
        renderStatusBar(ctx);
      }, 120);
      spinnerTimer.unref?.();
      subscription = subscribe(
        controlSocket,
        (record: any) => {
          if (record.type === "response" && record.success && record.data?.tasks) {
            snapshot = record.data;
            refreshWidget(ctx);
          } else if (record.type === "activity") {
            snapshot.activities = { ...snapshot.activities, [record.activity.taskId]: record.activity };
            refreshWidget(ctx);
          } else {
            void refreshSnapshot(ctx);
          }
          void maybeClaimReview();
        },
        (error: Error) => ctx.ui.notify(`Orchestrator stream reconnecting: ${error.message}`, "warning"),
      );
      void maybeClaimReview();
    } catch (error) {
      ctx.ui.notify(`Orchestrator unavailable: ${error instanceof Error ? error.message : String(error)}`, "error");
    }
  });

  pi.on("agent_settled", async () => void maybeClaimReview());
  pi.on("session_shutdown", async () => {
    subscription?.end();
    subscription = undefined;
    if (spinnerTimer) clearInterval(spinnerTimer);
    spinnerTimer = undefined;
    activeContext?.ui.setWidget("graft-orchestrator", undefined);
    activeContext?.ui.setStatus("graft-orchestrator", undefined);
    activeContext?.ui.setFooter(undefined);
    activeContext = undefined;
    dashboard = undefined;
  });

  pi.registerTool({
    name: "orch_review_action",
    label: "Orchestrator Review Action",
    description: "Resolve one claimed subagent result after reviewing its evidence. Follow up, ask the maintainer, request PR approval, complete, or block.",
    promptSnippet: "Resolve the current orchestrator review item",
    promptGuidelines: [
      "Use orch_review_action exactly once after reviewing a graft-orchestrator-review message; never infer task acceptance merely from agent_settled.",
      "Use orch_review_action action ask_maintainer for product/policy decisions and request_pr before any push or PR action.",
    ],
    parameters: Type.Object({
      taskId: Type.String(),
      action: StringEnum(["follow_up", "ask_maintainer", "request_pr", "complete", "blocked"] as const),
      message: Type.Optional(Type.String()),
    }),
    async execute(_id, params) {
      await request(controlSocket, { type: "review_action", taskId: params.taskId, action: params.action, message: params.message });
      if (activeContext) await refreshSnapshot(activeContext);
      return { content: [{ type: "text", text: `Orchestrator review action recorded: ${params.action}` }], details: params };
    },
  });

  pi.registerTool({
    name: "orch_backlog_draft",
    label: "Draft Ordered Backlog",
    description: "Queue a fresh read-only Terra agent to split a broad request into small ordered backlog tickets. This only creates a local draft; GitHub creation requires a separate maintainer approval.",
    parameters: Type.Object({
      request: Type.String({ minLength: 1, maxLength: 12000 }),
      maxTickets: Type.Optional(Type.Integer({ minimum: 1, maximum: 10 })),
    }),
    async execute(_id, params) {
      const data: any = await request(controlSocket, { type: "draft_backlog", request: params.request, maxTickets: params.maxTickets, requestedBy: "orchestrator-tool" });
      if (activeContext) await refreshSnapshot(activeContext);
      return {
        content: [{ type: "text", text: `Backlog splitter queued as ${data.taskId}. No GitHub issue has been created; the complete ordered batch will require maintainer approval.` }],
        details: data,
      };
    },
  });

  pi.registerTool({
    name: "orch_status",
    label: "Orchestrator Status",
    description: "Read a compact durable orchestrator status summary without dumping prompts, results, or mailboxes.",
    parameters: Type.Object({}),
    async execute() {
      const data: any = await request(controlSocket, { type: "snapshot" });
      const tasks = data.tasks ?? [];
      const active = tasks
        .filter((task: any) => ["dispatching", "running", "scanning"].includes(task.state))
        .map((task: any) => ({
          id: task.id,
          ticket: task.ticket,
          phase: task.phase,
          state: task.state,
          model: task.model,
          thinking: task.thinkingLevel ?? task.nextThinking ?? task.thinking,
          activity: data.activities?.[task.id]?.tool ?? data.activities?.[task.id]?.phase,
        }));
      const summary = {
        timestamp: data.timestamp,
        active,
        queued: tasks.filter((task: any) => task.state === "queued").map((task: any) => ({ id: task.id, ticket: task.ticket, phase: task.phase })),
        reviewQueue: tasks.filter((task: any) => task.state === "awaiting_review" || ["review_deferred", "preparing_review"].includes(task.state) || (task.profile === "change-review" && ["queued", "dispatching", "running"].includes(task.state))).length,
        userActions: maintainerItems(data).length,
        plans: (data.plans ?? []).map((plan: any) => ({ id: plan.id, ticket: plan.ticket, state: plan.state })),
      };
      return { content: [{ type: "text", text: JSON.stringify(summary, null, 2) }], details: summary };
    },
  });

  pi.registerCommand("backlog", {
    description: "Draft a small ordered GitHub backlog batch (usage: /backlog <request>)",
    handler: async (args, ctx) => {
      const backlogRequest = args.trim();
      if (!backlogRequest) {
        ctx.ui.notify("Usage: /backlog <request to split into small tickets>", "warning");
        return;
      }
      try {
        await ensureDaemon(ctx);
        const data: any = await request(controlSocket, { type: "draft_backlog", request: backlogRequest, maxTickets: 6, requestedBy: "maintainer-command" });
        await refreshSnapshot(ctx);
        ctx.ui.notify(`Backlog splitter queued: ${data.taskId}. GitHub creation still requires your approval.`, "success");
      } catch (error) {
        ctx.ui.notify(`Backlog draft failed: ${error instanceof Error ? error.message : String(error)}`, "error");
      }
    },
  });

  pi.registerCommand("orch", {
    description: "Open the colored orchestrator action list and live dashboard",
    handler: async (_args, ctx) => {
      snapshot = await request(controlSocket, { type: "snapshot" });
      refreshWidget(ctx);
      const dashboardOptions: any = {
        overlay: true,
        overlayOptions: { width: "88%", minWidth: 58, maxHeight: "86%", anchor: "center", margin: 1 },
      };
      const modalOptions: any = {
        overlay: true,
        overlayOptions: { width: "76%", minWidth: 52, maxHeight: "78%", anchor: "center", margin: 2 },
      };
      while (true) {
        try {
          const selected: any = await ctx.ui.custom((tui, theme, _keybindings, done) => {
            dashboard = new OrchDashboard(tui, theme, () => snapshot, done);
            return dashboard;
          }, dashboardOptions);
          dashboard = undefined;
          if (!selected) return;
          if (selected.kind === "refresh") {
            await request(controlSocket, { type: "refresh_admissions" });
            await refreshSnapshot(ctx);
            ctx.ui.notify("GitHub action list refreshed", "info");
            continue;
          }
          if (selected.kind === "open-window") {
            const task = selected.item.detail;
            const viewerName = `view-${task.id}`;
            const target = `${tmuxSession}:${viewerName}`;
            const exists = await pi.exec("tmux", ["has-session", "-t", target], { timeout: 3000 });
            if (exists.code !== 0) {
              const viewerPath = resolve(ctx.cwd, ".pi/extensions/graft-orchestrator/viewer.mjs");
              const command = `exec ${shellQuote(process.execPath)} ${shellQuote(viewerPath)} --socket ${shellQuote(controlSocket)} --task ${shellQuote(task.id)}`;
              const launched = await pi.exec("tmux", ["new-window", "-d", "-t", tmuxSession, "-n", viewerName, command], { timeout: 5000 });
              if (launched.code !== 0) {
                ctx.ui.notify(`Cannot open agent view: ${launched.stderr}`, "error");
                continue;
              }
            }
            await pi.exec("tmux", ["select-window", "-t", target], { timeout: 3000 });
            return;
          }
          const item = selected.item;
          const answer: string | null = await ctx.ui.custom(
            (tui, theme, _keybindings, done) => new OrchDetailModal(tui, theme, item, done),
            modalOptions,
          );
          if (!answer) continue;
          let explanation: string | null = null;
          if (answer === "explain") {
            explanation = await ctx.ui.custom(
              (tui, theme, _keybindings, done) => new ExplanationModal(tui, theme, "Explanation for the Orchestrator", done),
              modalOptions,
            );
            if (!explanation) continue;
          }
          if (item.kind === "admission") {
            if (answer === "yes") {
              await request(controlSocket, { type: "approve_admission", ticket: item.detail.ticket }, 45000);
              ctx.ui.notify(`Ticket #${item.detail.ticket} admitted`, "success");
            } else {
              await request(controlSocket, { type: "park_admission", ticket: item.detail.ticket, explanation });
              ctx.ui.notify(`Ticket #${item.detail.ticket} parked${explanation ? " with an explanation" : ""}`, "info");
            }
          } else if (item.kind === "decision") {
            await request(controlSocket, {
              type: "resolve_decision",
              decisionId: item.detail.id,
              answer: answer === "explain" ? "explanation" : answer,
              explanation,
            });
            ctx.ui.notify("Decision saved durably", "success");
          }
        } catch (error) {
          dashboard = undefined;
          ctx.ui.notify(`Orchestrator dashboard: ${error instanceof Error ? error.message : String(error)}`, "error");
          return;
        }
      }
    },
  });
}
