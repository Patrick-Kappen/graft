import assert from "node:assert/strict";
import { runScanPlan, scanPlan } from "../preflight.mjs";

const plan = scanPlan("required-local-ci");
assert.ok(plan.some((scan) => scan.id === "rust-static" && scan.phase === "quick"));
assert.ok(plan.some((scan) => scan.id === "coverage" && scan.phase === "full"));
assert.ok(plan.some((scan) => scan.id === "security"));
assert.ok(plan.some((scan) => scan.id === "nix-build"));
assert.throws(() => scanPlan("test-green"), /unknown preflight/);

const scans = [
  { id: "quick-green", label: "quick green", phase: "quick" },
  { id: "quick-red", label: "quick red", phase: "quick" },
  { id: "full", label: "full", phase: "full" },
];
const redCalls = [];
const red = await runScanPlan(scans, async (scan) => {
  redCalls.push(scan.id);
  return { code: scan.id === "quick-red" ? 9 : 0, stdout: "", stderr: "" };
});
assert.equal(red.passed, false);
assert.deepEqual(redCalls, ["quick-green", "quick-red"], "a red quick phase must return privately without spending resources on full scans");

const greenCalls = [];
const green = await runScanPlan(scans, async (scan) => {
  greenCalls.push(scan.id);
  return { code: 0, stdout: "", stderr: "" };
});
assert.equal(green.passed, true);
assert.deepEqual(greenCalls, ["quick-green", "quick-red", "full"]);
console.log("automatic preflight scan-plan test passed");
