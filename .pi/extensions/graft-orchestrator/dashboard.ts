import type { Theme } from "@earendil-works/pi-coding-agent";
import { Editor, Key, matchesKey, type EditorTheme, truncateToWidth, visibleWidth, wrapTextWithAnsi } from "@earendil-works/pi-tui";

export type DashboardTab = "inbox" | "active" | "all" | "usage";
export type DashboardItem = {
  kind: "admission" | "decision" | "task";
  id: string;
  title: string;
  subtitle: string;
  state: string;
  detail: any;
};

function pending(snapshot: any, collection: string) {
  return (snapshot?.[collection] ?? []).filter((item: any) => item.state === "pending");
}

export function maintainerItems(snapshot: any): DashboardItem[] {
  const items: DashboardItem[] = [];
  for (const admission of pending(snapshot, "admissions")) {
    items.push({
      kind: "admission",
      id: String(admission.ticket),
      title: `Admit ticket #${admission.ticket}?`,
      subtitle: admission.title ?? "Untitled GitHub ticket",
      state: "action needed",
      detail: admission,
    });
  }
  for (const decision of pending(snapshot, "decisions")) {
    const draft = (snapshot?.backlogDrafts ?? []).find((item: any) => item.id === decision.draftId);
    items.push({
      kind: "decision",
      id: decision.id,
      title: decision.kind === "pr" ? "PR approval required" : decision.title ?? "Decision required",
      subtitle: decision.question ?? "No explanation provided",
      state: decision.kind === "pr" ? "PR" : "decision",
      detail: draft ? { ...decision, draft } : decision,
    });
  }
  return items;
}

function taskItem(snapshot: any, task: any): DashboardItem {
  const activity = snapshot.activities?.[task.id];
  const live = activity?.tool ? `${activity.phase ?? "tool"}: ${activity.tool}` : activity?.phase;
  return {
    kind: "task",
    id: task.id,
    title: `${task.ticket ? `#${task.ticket}` : "Backlog"} · ${task.id}`,
    subtitle: live ?? task.history?.at(-1)?.detail ?? "No recent activity",
    state: task.state,
    detail: { ...task, activity },
  };
}

export function dashboardItems(snapshot: any, tab: DashboardTab): DashboardItem[] {
  if (tab === "usage") return [];
  if (tab === "inbox") return maintainerItems(snapshot);
  const tasks = snapshot?.tasks ?? [];
  if (tab === "active") {
    return tasks
      .filter((task: any) => ["queued", "dispatching", "running", "scanning", "review_deferred", "preparing_review", "waiting_review_agent", "awaiting_review", "waiting_decision", "waiting_pr_approval"].includes(task.state))
      .map((task: any) => taskItem(snapshot, task));
  }
  return [...tasks]
    .sort((left: any, right: any) => Number(left.state === "done") - Number(right.state === "done"))
    .map((task: any) => taskItem(snapshot, task));
}

function stateColor(state: string): "success" | "warning" | "error" | "accent" | "muted" {
  if (["done", "success", "active"].includes(state)) return "success";
  if (["failed", "blocked"].includes(state)) return "error";
  if (["running", "dispatching", "scanning", "review_deferred", "preparing_review", "waiting_review_agent"].includes(state)) return "accent";
  if (["action needed", "decision", "PR", "waiting_decision", "waiting_pr_approval", "awaiting_review"].includes(state)) return "warning";
  return "muted";
}

function displayState(state: string) {
  const labels: Record<string, string> = {
    review_deferred: "review deferred",
    preparing_review: "preparing fresh review",
    waiting_review_agent: "fresh review queued",
    awaiting_review: "awaiting exceptional review",
    waiting_decision: "waiting for user",
    waiting_pr_approval: "PR approval",
    waiting_pr_action: "PR action",
    scanning: "automatic scans",
  };
  return labels[state] ?? state.replaceAll("_", " ");
}

function formatTokens(value: number) {
  if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(2)}M`;
  if (value >= 1_000) return `${(value / 1_000).toFixed(1)}K`;
  return String(value ?? 0);
}

function usageText(usage: any) {
  const prompt = (usage?.input ?? 0) + (usage?.cacheRead ?? 0) + (usage?.cacheWrite ?? 0);
  const cacheRate = prompt > 0 ? ((usage.cacheRead ?? 0) / prompt) * 100 : 0;
  return `${formatTokens(usage?.totalTokens ?? 0)} tok · in ${formatTokens(usage?.input ?? 0)} · out ${formatTokens(usage?.output ?? 0)} · cache ${formatTokens((usage?.cacheRead ?? 0) + (usage?.cacheWrite ?? 0))} (${cacheRate.toFixed(1)}%) · $${(usage?.costUsd ?? 0).toFixed(4)} · €≈${(usage?.costEur ?? 0).toFixed(4)}`;
}

function crop(text: string, max = 180) {
  const clean = String(text ?? "").replaceAll(/\s+/g, " ").trim();
  return clean.length <= max ? clean : `${clean.slice(0, max - 1)}…`;
}

function bordered(theme: Theme, width: number, body: string[], color: "accent" | "warning" | "success" = "accent") {
  const w = Math.max(32, width);
  const inner = w - 2;
  const pad = (text: string) => {
    const clipped = truncateToWidth(text, inner);
    return clipped + " ".repeat(Math.max(0, inner - visibleWidth(clipped)));
  };
  const row = (text = "") => `${theme.fg(color, "│")}${pad(text)}${theme.fg(color, "│")}`;
  return [theme.fg(color, `╭${"─".repeat(inner)}╮`), ...body.map(row), theme.fg(color, `╰${"─".repeat(inner)}╯`)];
}

export class OrchDashboard {
  private selected = 0;
  private tab: DashboardTab = "inbox";
  constructor(
    private tui: any,
    private theme: Theme,
    private getSnapshot: () => any,
    private done: (value: any) => void,
  ) {}

  invalidate() {}

  refresh() {
    const size = dashboardItems(this.getSnapshot(), this.tab).length;
    this.selected = Math.min(this.selected, Math.max(0, size - 1));
    this.tui.requestRender();
  }

  private setTab(tab: DashboardTab) {
    this.tab = tab;
    this.selected = 0;
    this.refresh();
  }

  handleInput(data: string) {
    const items = dashboardItems(this.getSnapshot(), this.tab);
    if (data === "1") return this.setTab("inbox");
    if (data === "2") return this.setTab("active");
    if (data === "3") return this.setTab("all");
    if (data === "4") return this.setTab("usage");
    if (data === "o" && items[this.selected]?.kind === "task") return this.done({ kind: "open-window", item: items[this.selected] });
    const tabs: DashboardTab[] = ["inbox", "active", "all", "usage"];
    if (matchesKey(data, Key.left)) return this.setTab(tabs[Math.max(0, tabs.indexOf(this.tab) - 1)]);
    if (matchesKey(data, Key.right)) return this.setTab(tabs[Math.min(tabs.length - 1, tabs.indexOf(this.tab) + 1)]);
    if (matchesKey(data, Key.up)) this.selected = Math.max(0, this.selected - 1);
    else if (matchesKey(data, Key.down)) this.selected = Math.min(Math.max(0, items.length - 1), this.selected + 1);
    else if (matchesKey(data, Key.enter) && items[this.selected]) return this.done({ kind: "open", item: items[this.selected] });
    else if (matchesKey(data, Key.escape) || data === "q") return this.done(null);
    else if (data === "r") return this.done({ kind: "refresh" });
    this.tui.requestRender();
  }

  render(width: number) {
    const data = this.getSnapshot();
    const actions = maintainerItems(data);
    const active = dashboardItems(data, "active");
    const items = dashboardItems(data, this.tab);
    const usable = Math.max(32, width);
    const inner = usable - 2;
    const fit = (text: string) => {
      const wrapped = wrapTextWithAnsi(text, Math.max(1, inner - 2));
      return truncateToWidth(wrapped[0] ?? "", Math.max(1, inner - 2));
    };
    const tab = (key: string, id: DashboardTab, label: string, count: number) => {
      const text = `[${key}] ${label} ${count}`;
      return id === this.tab ? this.theme.fg("accent", this.theme.bold(text)) : this.theme.fg("muted", text);
    };
    const body: string[] = [
      ` ${this.theme.fg("accent", this.theme.bold("Graft Orchestrator"))}  ${actions.length ? this.theme.fg("warning", `${actions.length} action${actions.length === 1 ? "" : "s"} for you`) : this.theme.fg("success", "no action required")}`,
      ` ${tab("1", "inbox", "My Actions", actions.length)}   ${tab("2", "active", "Active", active.length)}   ${tab("3", "all", "All", (data.tasks ?? []).length)}   ${tab("4", "usage", "Usage", data.usageSummary?.total?.turns ?? 0)}`,
      ` ${this.theme.fg("dim", "↑↓ select · Enter details · o agent window · ←→ tab · r refresh · Esc close")}`,
      ` ${this.theme.fg("border", "─".repeat(Math.max(1, inner - 2)))}`,
    ];
    if (this.tab === "usage") {
      const summary = data.usageSummary ?? { total: {}, phases: [], agents: [], currencyNote: "No usage data" };
      body.push("", ` ${this.theme.fg("accent", this.theme.bold("Total model usage"))}`, ` ${fit(usageText(summary.total))}`);
      body.push("", ` ${this.theme.fg("accent", this.theme.bold("By phase"))}`);
      for (const phase of summary.phases ?? []) {
        body.push(` ${this.theme.fg("text", this.theme.bold(phase.id))}  ${this.theme.fg("muted", fit(usageText(phase.usage)))}`);
        body.push(`   ${this.theme.fg("dim", fit((phase.agents ?? []).join(" · ")))}`);
      }
      body.push("", ` ${this.theme.fg("accent", this.theme.bold("By agent and route"))}`);
      for (const agent of summary.agents ?? []) {
        body.push(` ${this.theme.fg("text", this.theme.bold(agent.id))}`);
        body.push(`   ${this.theme.fg("muted", fit(usageText(agent.usage)))}`);
      }
      body.push("", ` ${this.theme.fg("dim", fit(summary.currencyNote ?? "Pi-reported nominal model cost"))}`);
      return bordered(this.theme, usable, body, "accent");
    }
    if (!items.length) {
      body.push("", ` ${this.theme.fg("success", "✓ All clear — nothing needs your attention.")}`, "");
    } else {
      const start = Math.max(0, Math.min(this.selected - 5, Math.max(0, items.length - 10)));
      const shown = items.slice(start, start + 10);
      for (let offset = 0; offset < shown.length; offset++) {
        const index = start + offset;
        const item = shown[offset];
        const selected = index === this.selected;
        const marker = selected ? this.theme.fg("accent", "▸") : " ";
        const badge = this.theme.fg(stateColor(item.state), this.theme.bold(displayState(item.state).toUpperCase()));
        const title = selected ? this.theme.fg("accent", this.theme.bold(item.title)) : this.theme.fg("text", item.title);
        body.push(` ${marker} ${badge}  ${fit(title)}`);
        body.push(`     ${this.theme.fg("muted", fit(item.subtitle))}`);
      }
      if (items.length > shown.length) body.push(` ${this.theme.fg("dim", `${this.selected + 1}/${items.length} · more items with ↑↓`)}`);
    }
    return bordered(this.theme, usable, body, actions.length && this.tab === "inbox" ? "warning" : "accent");
  }
}

function detailLines(theme: Theme, item: DashboardItem, width: number) {
  const lines: string[] = [];
  const add = (label: string, value: unknown, color: "text" | "muted" | "accent" = "text") => {
    if (value === undefined || value === null || value === "") return;
    const prefix = ` ${theme.fg("muted", `${label}:`)} `;
    const wrapped = wrapTextWithAnsi(theme.fg(color, String(value)), Math.max(10, width - visibleWidth(prefix) - 2));
    wrapped.forEach((line, index) => lines.push(`${index === 0 ? prefix : " ".repeat(visibleWidth(prefix))}${line}`));
  };
  if (item.kind === "admission") {
    add("Ticket", `#${item.detail.ticket}`);
    add("Title", item.detail.title, "accent");
    add("Agent", item.detail.agent ?? "selected on admission");
    add("Model", item.detail.model ?? "profile default");
    add("Scope", item.detail.scope ?? item.detail.profile ?? "ticket scope");
    add("URL", item.detail.url, "muted");
    lines.push("");
    add("Description", crop(item.detail.body, 900));
  } else if (item.kind === "decision") {
    add("Question", item.detail.question, "accent");
    add("Task", item.detail.taskId);
    add("Kind", item.detail.kind);
    if (item.detail.draft) {
      const plan = item.detail.draft.tickets.map((ticket: any, index: number) => [
        `${index + 1}. ${ticket.title}`,
        `Labels: ${ticket.labels.join(", ")}`,
        `Depends on: ${ticket.dependsOn.length ? ticket.dependsOn.join(", ") : "none"}`,
        ticket.body,
      ].join("\n")).join("\n\n");
      add("Exact ticket batch", crop(plan, 12000));
    }
  } else {
    add("Task", item.detail.id);
    add("Ticket", item.detail.ticket ? `#${item.detail.ticket}` : undefined);
    add("Status", displayState(item.detail.state), "accent");
    add("Live", item.detail.activity?.tool ? `${item.detail.activity.phase}: ${item.detail.activity.tool}` : item.detail.activity?.phase ?? "inactive");
    add("Model", item.detail.model ? `${item.detail.model}:${item.detail.thinkingLevel ?? item.detail.nextThinking ?? item.detail.thinking ?? "default"}` : undefined);
    add("Worktree", item.detail.worktree, "muted");
    add("Branch", item.detail.branch, "muted");
    add("Result", crop(item.detail.result?.finalText, 1200));
    const recent = (item.detail.history ?? []).slice(-5).map((entry: any) => `${displayState(entry.state)}: ${entry.detail}`).join("\n");
    add("History", recent, "muted");
  }
  return lines;
}

export class OrchDetailModal {
  private scroll = 0;
  private maxScroll = 0;
  constructor(private tui: any, private theme: Theme, private item: DashboardItem, private done: (value: string | null) => void) {}
  invalidate() {}
  handleInput(data: string) {
    if (matchesKey(data, Key.escape) || data === "q") return this.done(null);
    if (matchesKey(data, Key.up)) { this.scroll = Math.max(0, this.scroll - 1); this.tui.requestRender(); }
    else if (matchesKey(data, Key.down)) { this.scroll = Math.min(this.maxScroll, this.scroll + 1); this.tui.requestRender(); }
    else if (this.item.kind !== "task" && (data === "y" || data === "Y")) return this.done("yes");
    else if (this.item.kind !== "task" && (data === "n" || data === "N")) return this.done("no");
    else if (this.item.kind !== "task" && (data === "e" || data === "E")) return this.done("explain");
  }
  render(width: number) {
    const usable = Math.max(32, width);
    const detail = detailLines(this.theme, this.item, usable - 4);
    const pageSize = 24;
    this.maxScroll = Math.max(0, detail.length - pageSize);
    this.scroll = Math.min(this.scroll, this.maxScroll);
    const shown = detail.slice(this.scroll, this.scroll + pageSize);
    const body = [
      ` ${this.theme.fg("warning", this.theme.bold(this.item.kind === "task" ? "Task Details" : "Your Decision"))}`,
      ` ${this.theme.fg("accent", this.theme.bold(this.item.title))}`,
      ` ${this.theme.fg("dim", this.maxScroll ? `↑↓ scroll · lines ${this.scroll + 1}-${this.scroll + shown.length}/${detail.length}` : "")}`,
      "",
      ...shown,
      "",
    ];
    if (this.item.kind === "task") body.push(` ${this.theme.fg("dim", "↑↓ scroll · Esc close")}`);
    else body.push(` ${this.theme.fg("success", "[Y]es")}   ${this.theme.fg("error", "[N]o")}   ${this.theme.fg("accent", "[E]xplain")}   ${this.theme.fg("dim", "↑↓ scroll · Esc back")}`);
    return bordered(this.theme, usable, body, this.item.kind === "task" ? "accent" : "warning");
  }
}

export class ExplanationModal {
  private editor: Editor;
  constructor(private tui: any, private theme: Theme, private title: string, private done: (value: string | null) => void) {
    const editorTheme: EditorTheme = {
      borderColor: (text) => theme.fg("accent", text),
      selectList: {
        selectedPrefix: (text) => theme.fg("accent", text),
        selectedText: (text) => theme.fg("accent", text),
        description: (text) => theme.fg("muted", text),
        scrollInfo: (text) => theme.fg("dim", text),
        noMatch: (text) => theme.fg("warning", text),
      },
    };
    this.editor = new Editor(tui, editorTheme);
    this.editor.onSubmit = (value) => {
      const text = value.trim();
      if (text) done(text);
    };
  }
  invalidate() { this.editor.invalidate(); }
  handleInput(data: string) {
    if (matchesKey(data, Key.escape)) return this.done(null);
    this.editor.handleInput(data);
    this.tui.requestRender();
  }
  render(width: number) {
    const usable = Math.max(32, width);
    const body = [
      ` ${this.theme.fg("accent", this.theme.bold(this.title))}`,
      ` ${this.theme.fg("muted", "Add context or propose an alternative. Enter submits; Esc cancels.")}`,
      "",
      ...this.editor.render(Math.max(20, usable - 4)),
    ];
    return bordered(this.theme, usable, body, "accent");
  }
}
