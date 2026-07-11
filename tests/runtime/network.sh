#!/usr/bin/env bash
set -euo pipefail

if [[ $# -ne 1 ]]; then
  echo "usage: $0 <static-busybox-rootfs>" >&2
  exit 2
fi

source_rootfs=$1
tmp=$(mktemp -d)
owner="graft-network-owner-$$"
cleanup() {
  podman rm -f "${owner}" >/dev/null 2>&1 || true
  rm -rf "${tmp}"
}
trap cleanup EXIT

unavailable() {
  if [[ ${GRAFT_REQUIRE_NETWORK_RUNTIME:-0} == 1 ]]; then
    echo "required rootless Podman network runtime is unavailable: $1" >&2
    exit 1
  fi
  echo "SKIP: rootless Podman network runtime is unavailable: $1"
  exit 0
}

command -v podman >/dev/null || unavailable "podman is not installed"
if [[ ! -d ${XDG_RUNTIME_DIR:-} ]]; then
  export XDG_RUNTIME_DIR="${tmp}/runtime"
  mkdir -m 0700 "${XDG_RUNTIME_DIR}"
fi
if ! podman_info=$(podman info 2>&1); then
  [[ ${GRAFT_REQUIRE_NETWORK_RUNTIME:-0} != 1 ]] || printf '%s\n' "${podman_info}" >&2
  unavailable "podman info failed"
fi

for rootfs in probe none owner client conflict; do
  cp -aL "${source_rootfs}" "${tmp}/${rootfs}"
  chmod -R u+w "${tmp:?}/${rootfs}"
done

podman run --rm --cgroups=disabled --network none --rootfs "${tmp}/probe" /bin/true \
  >/dev/null 2>&1 || unavailable "a rootless no-network container could not start"

routes=$(podman run --rm --cgroups=disabled --network none --rootfs "${tmp}/none" /bin/ip route)
test -z "${routes}"
if podman run --rm --cgroups=disabled --network none --rootfs "${tmp}/none" \
  /bin/timeout 2 /bin/wget -qO- http://1.1.1.1; then
  echo "none network unexpectedly reached an external IP" >&2
  exit 1
fi
echo "none-network-ok"

podman run -d --cgroups=disabled --network none --name "${owner}" \
  --rootfs "${tmp}/owner" /bin/httpd -f -p 127.0.0.1:18081 -h /www >/dev/null
for _ in $(seq 1 20); do
  if output=$(podman run --rm --cgroups=disabled --network "container:${owner}" \
    --rootfs "${tmp}/client" /bin/wget -qO- http://127.0.0.1:18081 2>/dev/null); then
    test "${output}" = "shared-network-ok"
    break
  fi
  sleep 0.25
done
test "${output:-}" = "shared-network-ok"

if podman run --rm --cgroups=disabled --network "container:${owner}" \
  --rootfs "${tmp}/conflict" /bin/httpd -f -p 127.0.0.1:18081 -h /www; then
  echo "shared namespace unexpectedly allowed a duplicate loopback port bind" >&2
  exit 1
fi
echo "shared-container-network-ok"
