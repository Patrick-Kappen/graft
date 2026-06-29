import type { ExtensionAPI } from "@earendil-works/pi-coding-agent";
import { createBashTool } from "@earendil-works/pi-coding-agent";
import { Type } from "typebox";
import { spawn } from "node:child_process";

const defaultPackages = ["nodejs_22", "typescript", "jujutsu", "ripgrep"];

type DirenvAction = "ensure" | "add" | "exec";

type DirenvToolInput = {
  action: DirenvAction;
  target?: string;
  cwd?: string;
};

export default function (pi: ExtensionAPI) {
  const bashTool = createBashTool(process.cwd(), {
    spawnHook: ({ command, cwd, env }) => ({
      command: wrapCommand(command),
      cwd,
      env,
    }),
  });

  pi.registerTool({
    ...bashTool,
    execute: async (id, params, signal, onUpdate, _ctx) => bashTool.execute(id, params, signal, onUpdate),
  });

  pi.registerTool({
    name: "direnv",
    label: "Direnv",
    description: "Manage a local persistent Nix dev shell for Pi. Success is terse; failures include the error message.",
    promptSnippet: "Use direnv to prepare local packages and run commands inside the project dev shell.",
    promptGuidelines: [
      "Use direnv for local agent tooling that should persist on this machine but not enter the repo.",
      "Use action=add for missing local tools, then normal bash commands can use them automatically.",
      "Do not add project dependencies to tracked files unless the user explicitly asks.",
    ],
    parameters: Type.Object({
      action: Type.Union([Type.Literal("ensure"), Type.Literal("add"), Type.Literal("exec")]),
      target: Type.Optional(Type.String({ description: "Package list for add, or shell command for exec. Not needed for ensure." })),
      cwd: Type.Optional(Type.String({ description: "Directory to run from. Defaults to Pi cwd." })),
    }),
    async execute(_toolCallId, params: DirenvToolInput, signal, _onUpdate, ctx) {
      const cwd = params.cwd ?? ctx.cwd;
      const result = await runScript(scriptFor(params), cwd, signal);

      if (result.ok) {
        return {
          content: [{ type: "text", text: "Direnv = ok" }],
          details: { ok: true, action: params.action },
        };
      }

      const message = result.message ? `\n${result.message}` : "";
      return {
        content: [{ type: "text", text: `Direnv = niet ok${message}` }],
        details: { ok: false, action: params.action, code: result.code },
        isError: true,
      };
    },
  });
}

function scriptFor(input: DirenvToolInput): string {
  if (input.action === "ensure") {
    return ensureScript();
  }

  if (input.action === "add") {
    if (!input.target) throw new Error("target is required for add");
    const packages = input.target.split(/\s+/).filter(Boolean);
    return `${ensureScript()}\n${addPackagesScript(packages)}`;
  }

  if (input.action === "exec") {
    if (!input.target) throw new Error("target is required for exec");
    return `${ensureScript()}\nnix develop "$DIRENV_SHELL_DIR" -c bash -lc ${shellQuote(input.target)}`;
  }

  throw new Error(`Unknown action: ${String(input.action)}`);
}

function wrapCommand(command: string): string {
  return `${ensureScript()}
nix develop "$DIRENV_SHELL_DIR" -c bash -lc ${shellQuote(command)}`;
}

function ensureScript(): string {
  return String.raw`set -euo pipefail
find_workspace_dir() {
  current="$1"
  while [ "$current" != / ]; do
    if [ -d "$current/.jj" ] || [ -d "$current/.git" ] || [ -f "$current/package.json" ] || [ -f "$current/flake.nix" ]; then
      printf '%s\n' "$current"
      return 0
    fi
    current="$(dirname "$current")"
  done
  return 1
}

find_project_root() {
  printf '%s\n' "$1"
}

ensure_exclude() {
  root="$1"
  if [ -f "$root/.git/info/exclude" ]; then
    for item in .envrc .direnv/ .pi-direnv/; do
      grep -qxF "$item" "$root/.git/info/exclude" || printf '%s\n' "$item" >> "$root/.git/info/exclude"
    done
  fi
}

write_packages_json() {
  file="$1"
  shift
  printf '{\n  "packages": [\n' > "$file"
  first=1
  for pkg in "$@"; do
    validate_package "$pkg"
    if [ "$first" -eq 0 ]; then printf ',\n' >> "$file"; fi
    first=0
    printf '    "%s"' "$pkg" >> "$file"
  done
  printf '\n  ]\n}\n' >> "$file"
}

write_flake() {
  packages_file="$1"
  flake_file="$2"
  packages="$(node -e 'const fs=require("fs"); const p=JSON.parse(fs.readFileSync(process.argv[1], "utf8")).packages; for (const x of [...new Set(p)].sort()) { if (!/^[A-Za-z0-9_.-]+$/.test(x)) process.exit(2); console.log(x); }' "$packages_file")"
  {
    printf '{\n'
    printf '  description = "Local Pi direnv shell";\n\n'
    printf '  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-26.05";\n\n'
    printf '  outputs = { nixpkgs, ... }:\n'
    printf '    let\n'
    printf '      systems = [ "x86_64-linux" "aarch64-linux" ];\n'
    printf '      forAllSystems = f: nixpkgs.lib.genAttrs systems (system: f (import nixpkgs { inherit system; }));\n'
    printf '    in {\n'
    printf '      devShells = forAllSystems (pkgs: {\n'
    printf '        default = pkgs.mkShell {\n'
    printf '          packages = [\n'
    for pkg in $packages; do printf '            pkgs.%s\n' "$pkg"; done
    printf '          ];\n'
    printf '        };\n'
    printf '      });\n'
    printf '    };\n'
    printf '}\n'
  } > "$flake_file"
}

validate_package() {
  case "$1" in
    ""|.*|*..*|*/*|*\\*) return 1 ;;
  esac
  printf '%s' "$1" | grep -Eq '^[A-Za-z0-9_.-]+$'
}

DIRENV_WORKSPACE_DIR="$(find_workspace_dir "$PWD")"
DIRENV_PROJECT_ROOT="$(find_project_root "$DIRENV_WORKSPACE_DIR")"
DIRENV_SHELL_DIR="$DIRENV_PROJECT_ROOT/.pi-direnv"
mkdir -p "$DIRENV_SHELL_DIR"
if [ ! -f "$DIRENV_SHELL_DIR/packages.json" ]; then
  write_packages_json "$DIRENV_SHELL_DIR/packages.json" ${defaultPackages.map(shellQuote).join(" ")}
fi
write_flake "$DIRENV_SHELL_DIR/packages.json" "$DIRENV_SHELL_DIR/flake.nix"
printf 'use flake ./.pi-direnv\n' > "$DIRENV_PROJECT_ROOT/.envrc"
ensure_exclude "$DIRENV_PROJECT_ROOT"`;
}
}

function addPackagesScript(packages: string[]): string {
  if (packages.length === 0) throw new Error("At least one package is required");
  for (const packageName of packages) validatePackageName(packageName);

  return `node -e ${shellQuote(`
const fs = require("fs");
const file = process.env.DIRENV_SHELL_DIR + "/packages.json";
const current = JSON.parse(fs.readFileSync(file, "utf8")).packages ?? [];
const added = ${JSON.stringify(packages)};
const packages = [...new Set([...current, ...added])].sort();
fs.writeFileSync(file, JSON.stringify({ packages }, null, 2) + "\\n");
`)}
write_flake "$DIRENV_SHELL_DIR/packages.json" "$DIRENV_SHELL_DIR/flake.nix"`;
}

function validatePackageName(packageName: string): void {
  if (!/^[A-Za-z0-9_.-]+$/.test(packageName) || packageName.startsWith(".") || packageName.endsWith(".")) {
    throw new Error(`Invalid package name: ${packageName}`);
  }
}

function shellQuote(value: string): string {
  return `'${value.replaceAll("'", "'\\''")}'`;
}

function runScript(script: string, cwd: string, signal?: AbortSignal): Promise<{ ok: boolean; code: number | null; message?: string }> {
  return new Promise((resolve, reject) => {
    let stdout = "";
    let stderr = "";
    const child = spawn("bash", ["-lc", script], { cwd, stdio: ["ignore", "pipe", "pipe"], signal });

    child.stdout?.setEncoding("utf8");
    child.stderr?.setEncoding("utf8");
    child.stdout?.on("data", (chunk) => { stdout += chunk; });
    child.stderr?.on("data", (chunk) => { stderr += chunk; });
    child.on("error", reject);
    child.on("exit", (code) => {
      const ok = code === 0;
      resolve({ ok, code, message: ok ? undefined : failureMessage(stderr, stdout) });
    });
  });
}

function failureMessage(stderr: string, stdout: string): string {
  const text = (stderr.trim() || stdout.trim()).trim();
  if (!text) return "No error output.";
  const maxLength = 2_000;
  return text.length <= maxLength ? text : `${text.slice(0, maxLength)}\n… truncated`;
}
