#!/usr/bin/env bash
# Issue #368 debug-attribution trial runner (TEST-ONLY).
# Recreates the exact control reproducer protocol with ONE deviation
# (user manager LogLevel=debug). Runs up to 3 independent 16-reboot
# trials sequentially; stops after the first readiness wedge.
#
# ARCHIVED RECORD. Kept for provenance, not runnable as-is: the paths below
# point at the scratch worktree /home/zerodawn/graft-v04-debug-repro, which no
# longer exists, and DRIVER is a store path from that run. Adjust both before
# reusing. The test itself now lives at tests/nixos/stress-repro.nix and is
# exposed as the flake check rootless-sdnotify-debug-log-trial.
set -u
cd /home/zerodawn/graft-v04-debug-repro || exit 2
DRIVER=/nix/store/yby5lmxd3ps2hc5dvsx17m487c86rr1d-nixos-test-driver-graft-rootless-sdnotify-repro-debug/bin/nixos-test-driver
SHA=8a9f2cf1ef2962f087c230ea64b2497c0d830f8c

echo "=== debug-trial launch $(date -u +%FT%TZ) SHA=$SHA ==="
echo "=== driver: $DRIVER ==="

for trial in 1 2 3; do
  RESULT_DIR="/home/zerodawn/graft-v04-debug-repro/run-trial${trial}-result"
  LOG="/home/zerodawn/graft-v04-debug-repro/run-trial${trial}.log"
  rm -rf "$RESULT_DIR"; mkdir -p "$RESULT_DIR"
  echo "=== trial $trial start $(date -u +%FT%TZ) ==="
  env PYTHONUNBUFFERED=1 timeout 2700 "$DRIVER" -o "$RESULT_DIR" > "$LOG" 2>&1
  rc=$?
  echo "=== trial $trial driver exit=$rc $(date -u +%FT%TZ) ==="
  if grep -Eq 'linger-probe\.service did not reach active or failed within 120s' "$LOG"; then
    echo "=== TRIAL $trial: READINESS WEDGE (stop early) ==="
    exit 1
  fi
  if grep -Eq 'test script finished' "$LOG"; then
    echo "=== TRIAL $trial: PASS ==="
  else
    echo "=== TRIAL $trial: UNEXPECTED OUTCOME (rc=$rc) — inspect $LOG ==="
    exit 2
  fi
done
echo "=== all 3 trials passed: no debug failure observed ==="