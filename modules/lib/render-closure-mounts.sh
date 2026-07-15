#!/usr/bin/env bash
set -euo pipefail

actual_paths=$1
rootfs=$2
template=$3
output=$4
option_name=$5
container_name=$6
marker=$7
limit_checker=$8

mount_lines=$(mktemp)
trap 'rm -f "$mount_lines"' EXIT

: > "$mount_lines"
bash "$limit_checker" "$actual_paths" "$mount_lines" "$option_name" "$container_name"
printf 'Volume=%s/nix/store:/nix/store:ro,bind,nodev,nosuid\n' "$rootfs" > "$mount_lines"

while IFS= read -r source_path; do
  case "$source_path" in
    /nix/store/*) ;;
    *)
      echo "$option_name: invalid final closure path '$source_path' for container '$container_name'" >&2
      exit 1
      ;;
  esac

  store_name=${source_path#/nix/store/}
  case "$store_name" in
    "" | */*)
      echo "$option_name: final closure path '$source_path' is not a direct store child for container '$container_name'" >&2
      exit 1
      ;;
  esac

  target_path="$rootfs/nix/store/$store_name"
  if [[ -L "$source_path" ]]; then
    echo "$option_name: top-level closure symlink '$source_path' is unsupported for container '$container_name'" >&2
    exit 1
  elif [[ -d "$source_path" ]]; then
    if [[ ! -d "$target_path" || -L "$target_path" ]]; then
      echo "$option_name: closure directory '$source_path' has no matching rootfs target for container '$container_name'" >&2
      exit 1
    fi
  elif [[ -f "$source_path" ]]; then
    if [[ ! -f "$target_path" || -L "$target_path" ]]; then
      echo "$option_name: closure file '$source_path' has no matching rootfs target for container '$container_name'" >&2
      exit 1
    fi
  else
    echo "$option_name: unsupported final closure object '$source_path' for container '$container_name'" >&2
    exit 1
  fi

  printf 'Volume=%s:%s:ro,bind,nodev,nosuid\n' "$source_path" "$source_path" >> "$mount_lines"
done < "$actual_paths"

bash "$limit_checker" "$actual_paths" "$mount_lines" "$option_name" "$container_name"

if [[ $(grep -Fxc "$marker" "$template") -ne 1 ]]; then
  echo "$option_name: Quadlet template for container '$container_name' lost its store-mount marker" >&2
  exit 1
fi

while IFS= read -r line; do
  if [[ "$line" == "$marker" ]]; then
    cat "$mount_lines"
  else
    printf '%s\n' "$line"
  fi
done < "$template" > "$output"
