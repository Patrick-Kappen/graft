import process from "node:process";

const requiredLocalCi = [
  {
    id: "workspace",
    label: "workspace integrity",
    phase: "quick",
    timeout: 60_000,
    command: "git",
    args: ["diff", "--check"],
  },
  {
    id: "workflow-lint",
    label: "workflow lint",
    phase: "quick",
    timeout: 10 * 60_000,
    command: "nix",
    args: ["develop", ".#ci", "-c", "bash", "-lc", [
      "set -euo pipefail",
      "actionlint",
      "zizmor --no-progress --color never --min-confidence high .github/workflows/*.yml .github/actions/setup-nix/action.yml",
    ].join("\n")],
  },
  {
    id: "rust-static",
    label: "Rust format and lint",
    phase: "quick",
    timeout: 20 * 60_000,
    command: "nix",
    args: ["develop", ".#ci", "-c", "bash", "-lc", [
      "set -euo pipefail",
      "cd crates/graft",
      "cargo fmt --check",
      "cargo clippy --all-targets -- -D warnings -D clippy::pedantic",
    ].join("\n")],
  },
  {
    id: "nix-static",
    label: "Nix and shell lint",
    phase: "quick",
    timeout: 20 * 60_000,
    command: "nix",
    args: ["develop", ".#ci", "-c", "bash", "-lc", [
      "set -euo pipefail",
      "git ls-files '*.nix' -z | xargs -0 nixfmt --check",
      "statix check .",
      "deadnix --fail .",
      "shellcheck modules/lib/materialise-rootfs-etc.sh tests/runtime/network.sh",
    ].join("\n")],
  },
  {
    id: "docs-static",
    label: "TOML and documentation lint",
    phase: "quick",
    timeout: 15 * 60_000,
    command: "nix",
    args: ["develop", ".#ci", "-c", "bash", "-lc", [
      "set -euo pipefail",
      "git ls-files '*.toml' -z | xargs -0 taplo format --check",
      "git ls-files '*.toml' -z | xargs -0 taplo lint",
      "git ls-files '*.md' -z | xargs -0 markdownlint-cli2 --config .markdownlint.jsonc",
      "git ls-files '*.md' | lychee --files-from - --offline --include-fragments --no-progress",
      "{ git ls-files '*.md'; git ls-files 'examples/quickstart/**'; } | typos --file-list -",
    ].join("\n")],
  },
  {
    id: "rust-tests",
    label: "Rust tests and structure",
    phase: "full",
    timeout: 30 * 60_000,
    command: "nix",
    args: ["develop", ".#ci", "-c", "bash", "-lc", [
      "set -euo pipefail",
      "cd crates/graft",
      "mkdir -p target/nextest",
      "cargo nextest run --profile ci",
      "cargo test --doc",
      "cargo machete",
      "NO_COLOR=1 cargo modules orphans --lib",
      "NO_COLOR=1 cargo modules orphans --bin graft-pause",
    ].join("\n")],
  },
  {
    id: "coverage",
    label: "Rust coverage threshold",
    phase: "full",
    timeout: 30 * 60_000,
    command: "nix",
    args: ["develop", ".#ci", "-c", "bash", "-lc", [
      "set -euo pipefail",
      "cd crates/graft",
      "mkdir -p target/coverage",
      "export LLVM_COV=$(command -v llvm-cov)",
      "export LLVM_PROFDATA=$(command -v llvm-profdata)",
      "cargo llvm-cov --workspace --all-features --fail-under-lines 80 --lcov --output-path target/coverage/lcov.info",
    ].join("\n")],
  },
  {
    id: "security",
    label: "dependency and secret scans",
    phase: "full",
    timeout: 25 * 60_000,
    command: "nix",
    args: ["develop", ".#ci", "-c", "bash", "-lc", [
      "set -euo pipefail",
      "(cd crates/graft && cargo-audit audit)",
      "cargo deny --manifest-path crates/graft/Cargo.toml check --config deny.toml",
      "scan_root=$(mktemp -d)",
      "cleanup() { rm -rf \"$scan_root\"; }",
      "trap cleanup EXIT",
      "git ls-files -co --exclude-standard -z | tar --null --files-from=- -cf - | tar -xf - -C \"$scan_root\"",
      "gitleaks dir --no-banner --no-color --redact \"$scan_root\"",
    ].join("\n")],
  },
  {
    id: "docs-build",
    label: "documentation build and drift",
    phase: "full",
    timeout: 25 * 60_000,
    command: "nix",
    args: ["develop", ".#ci", "-c", "bash", "-lc", [
      "set -euo pipefail",
      "./scripts/build-docs.sh",
      "nix build .#checks.x86_64-linux.documentation-drift --no-link --print-out-paths",
    ].join("\n")],
  },
  {
    id: "nix-build",
    label: "Nix package and safe required checks",
    phase: "full",
    timeout: 60 * 60_000,
    command: "bash",
    args: ["-lc", [
      "set -euo pipefail",
      "nix build .#packages.x86_64-linux.default --no-link --print-out-paths",
      "nix flake check --print-build-logs",
      "nix build .#packages.x86_64-linux.system-manifest-publication-runtime-test --no-link --print-build-logs",
      "nix build .#checks.x86_64-linux.network-runtime-rootfs --no-link --print-out-paths",
    ].join("\n")],
  },
];

const testPlans = {
  "test-green": [{ id: "test-green", label: "test green", phase: "quick", timeout: 5_000, command: process.execPath, args: ["-e", "process.exit(0)"] }],
  "test-red": [{ id: "test-red", label: "test red", phase: "quick", timeout: 5_000, command: process.execPath, args: ["-e", "console.error('synthetic red scan'); process.exit(7)"] }],
};

export function scanPlan(kind, options = {}) {
  if (kind === "required-local-ci") return requiredLocalCi.map((scan) => ({ ...scan, args: [...scan.args] }));
  if (options.allowTest && testPlans[kind]) return testPlans[kind].map((scan) => ({ ...scan, args: [...scan.args] }));
  throw new Error(`unknown preflight scan plan ${kind}`);
}

export async function runScanPlan(scans, execute, onProgress = async () => {}) {
  const results = [];
  for (const phase of ["quick", "full"]) {
    const phaseScans = scans.filter((scan) => scan.phase === phase);
    if (!phaseScans.length) continue;
    const phaseResults = [];
    for (const scan of phaseScans) {
      await onProgress({ type: "scan_start", scan });
      let result;
      try {
        result = await execute(scan);
      } catch (error) {
        result = { code: 1, stdout: "", stderr: error instanceof Error ? error.message : String(error) };
      }
      const recorded = {
        id: scan.id,
        label: scan.label,
        phase,
        code: Number.isInteger(result.code) ? result.code : 1,
        stdout: String(result.stdout ?? ""),
        stderr: String(result.stderr ?? ""),
        timedOut: Boolean(result.timedOut),
        logPath: result.logPath,
      };
      results.push(recorded);
      phaseResults.push(recorded);
      await onProgress({ type: "scan_end", scan, result: recorded });
    }
    if (phaseResults.some((result) => result.code !== 0)) break;
  }
  return { passed: results.length === scans.length && results.every((result) => result.code === 0), results };
}
