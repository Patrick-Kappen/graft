#!/usr/bin/env bash
set -euo pipefail

closure_file=$1
rootfs=$2
option_name=$3
container_name=$4

LC_ALL=C sort -u "$closure_file" | while IFS= read -r store_path; do
  case "$store_path" in
    /nix/store/*) ;;
    *)
      echo "$option_name: invalid closure path '$store_path' for container '$container_name'" >&2
      exit 1
      ;;
  esac

  store_name=${store_path#/nix/store/}
  case "$store_name" in
    "" | */*)
      echo "$option_name: closure path '$store_path' is not a direct store child for container '$container_name'" >&2
      exit 1
      ;;
  esac

  target_path="$rootfs/nix/store/$store_name"
  if [[ -L "$store_path" ]]; then
    echo "$option_name: top-level closure symlink '$store_path' is unsupported for container '$container_name'" >&2
    exit 1
  elif [[ -d "$store_path" ]]; then
    mkdir "$target_path"
  elif [[ -f "$store_path" ]]; then
    touch "$target_path"
  else
    echo "$option_name: unsupported closure object '$store_path' for container '$container_name'" >&2
    exit 1
  fi
done
