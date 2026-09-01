import { spawn } from "node:child_process";

const REPOSITORY = "Patrick-Kappen/graft";

export async function fetchIssueSnapshot(issueNumber) {
  if (!Number.isInteger(issueNumber) || issueNumber < 1) throw new Error("ticket must be a positive issue number");
  const stdout = await new Promise((resolve, reject) => {
    const child = spawn(
      "gh",
      ["issue", "view", String(issueNumber), "-R", REPOSITORY, "--json", "number,url,state,title,labels,assignees"],
      { shell: false, stdio: ["ignore", "pipe", "pipe"] },
    );
    let output = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => (output += chunk.toString("utf8")));
    child.stderr.on("data", (chunk) => (stderr += chunk.toString("utf8")));
    child.on("error", reject);
    child.on("exit", (code) => {
      if (code === 0) resolve(output);
      else reject(new Error(stderr.trim() || `gh issue view exited ${code}`));
    });
  });
  const issue = JSON.parse(stdout);
  if (issue.number !== issueNumber || typeof issue.state !== "string" || typeof issue.url !== "string") {
    throw new Error("GitHub returned an invalid issue snapshot");
  }
  return {
    issue: {
      number: issue.number,
      url: issue.url,
      state: issue.state,
      title: issue.title,
      labels: Array.isArray(issue.labels) ? issue.labels.map((label) => label.name).filter(Boolean) : [],
      assignees: Array.isArray(issue.assignees) ? issue.assignees.map((assignee) => assignee.login).filter(Boolean) : [],
    },
    fetchedAt: new Date().toISOString(),
  };
}

export async function listReadyP1Issues() {
  const stdout = await new Promise((resolve, reject) => {
    const child = spawn(
      "gh",
      ["issue", "list", "-R", REPOSITORY, "--state", "open", "--label", "priority:P1", "--label", "status:ready", "--json", "number,title,body,labels,url"],
      { shell: false, stdio: ["ignore", "pipe", "pipe"] },
    );
    let output = "";
    let stderr = "";
    child.stdout.on("data", (chunk) => (output += chunk.toString("utf8")));
    child.stderr.on("data", (chunk) => (stderr += chunk.toString("utf8")));
    child.on("error", reject);
    child.on("exit", (code) => (code === 0 ? resolve(output) : reject(new Error(stderr.trim() || `gh issue list exited ${code}`))));
  });
  const issues = JSON.parse(stdout);
  if (!Array.isArray(issues)) throw new Error("GitHub returned an invalid ready-ticket list");
  return issues.filter((issue) => Number.isInteger(issue.number) && typeof issue.title === "string");
}

export function isFresh(snapshot, maxAgeMs, now = Date.now()) {
  const fetchedAt = Date.parse(snapshot?.fetchedAt ?? "");
  return Number.isFinite(fetchedAt) && now - fetchedAt >= 0 && now - fetchedAt <= maxAgeMs;
}
