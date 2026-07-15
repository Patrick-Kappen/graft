#!/usr/bin/env bash
set -euo pipefail

closure_file=$1
mount_fragment=$2
option_name=$3
container_name=$4

member_count=$(wc -l < "$closure_file")
if [[ "$member_count" -gt 512 ]]; then
  echo "$option_name: closure for container '$container_name' has $member_count members; limit is 512; reduce config.runtime.packages" >&2
  exit 1
fi

fragment_size=$(wc -c < "$mount_fragment")
if [[ "$fragment_size" -gt 131072 ]]; then
  echo "$option_name: closure mount fragment for container '$container_name' is $fragment_size bytes; limit is 131072; reduce config.runtime.packages" >&2
  exit 1
fi
