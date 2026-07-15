#!/usr/bin/env bash
set -euo pipefail

inner=$1
rootfs=$2
option_name=$3
container_name=$4

source_etc="$inner/etc"
target_etc="$rootfs/etc"

if [[ -e "$source_etc" || -L "$source_etc" ]]; then
  if [[ ! -d "$source_etc" ]]; then
    echo "$option_name: package /etc is not a directory for container '$container_name'" >&2
    exit 1
  fi

  for reserved_name in mtab hostname hosts resolv.conf; do
    reserved_path="$source_etc/$reserved_name"
    if [[ -e "$reserved_path" || -L "$reserved_path" ]]; then
      echo "$option_name: package content conflicts with Graft-owned '/etc/$reserved_name' for container '$container_name'" >&2
      exit 1
    fi
  done

  if ! cp -rL -- "$source_etc/." "$target_etc/"; then
    echo "$option_name: failed to materialise package /etc for container '$container_name'" >&2
    exit 1
  fi
fi

if [[ ! -L "$target_etc/mtab" || $(readlink "$target_etc/mtab") != /proc/mounts ]]; then
  echo "$option_name: Graft-owned '/etc/mtab' is invalid for container '$container_name'" >&2
  exit 1
fi

for required_name in hostname hosts resolv.conf; do
  required_path="$target_etc/$required_name"
  if [[ ! -f "$required_path" || -L "$required_path" ]]; then
    echo "$option_name: Graft-owned '/etc/$required_name' is invalid for container '$container_name'" >&2
    exit 1
  fi
done

if [[ ! -f "$rootfs/run/.containerenv" || -L "$rootfs/run/.containerenv" ]]; then
  echo "$option_name: Graft-owned '/run/.containerenv' is invalid for container '$container_name'" >&2
  exit 1
fi
