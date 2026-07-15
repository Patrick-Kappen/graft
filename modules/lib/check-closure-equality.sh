#!/usr/bin/env bash
set -euo pipefail

expected_paths=$1
actual_paths=$2
option_name=$3
container_name=$4

if ! cmp -s "$expected_paths" "$actual_paths"; then
  echo "$option_name: final closure mismatch for container '$container_name'" >&2
  diff -u "$expected_paths" "$actual_paths" >&2 || true
  exit 1
fi
