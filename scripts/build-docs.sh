#!/usr/bin/env bash
set -euo pipefail

readonly MDBOOK_VERSION="0.5.2"
readonly MDBOOK_SHA256="084e4342ba564db270108763e404a7d1f309d932651a22484e93c0dc1a071f6d"
readonly MDBOOK_ARCHIVE="mdbook-v${MDBOOK_VERSION}-x86_64-unknown-linux-gnu.tar.gz"
readonly MDBOOK_URL="https://github.com/rust-lang/mdBook/releases/download/v${MDBOOK_VERSION}/${MDBOOK_ARCHIVE}"

if command -v mdbook >/dev/null 2>&1; then
  mdbook build
  exit 0
fi

if [[ "$(uname -s)" != "Linux" || "$(uname -m)" != "x86_64" ]]; then
  echo "mdbook is not installed and no pinned binary is configured for $(uname -s)/$(uname -m)." >&2
  echo "Use 'nix develop .#ci -c mdbook build' for local builds." >&2
  exit 1
fi

for command_name in curl sha256sum tar; do
  if ! command -v "${command_name}" >/dev/null 2>&1; then
    echo "Required build tool is missing: ${command_name}" >&2
    exit 1
  fi
done

work_dir="$(mktemp -d)"
trap 'rm -rf "${work_dir}"' EXIT

curl --fail --location --silent --show-error \
  --output "${work_dir}/${MDBOOK_ARCHIVE}" \
  "${MDBOOK_URL}"

echo "${MDBOOK_SHA256}  ${work_dir}/${MDBOOK_ARCHIVE}" | sha256sum --check --status

tar --extract --gzip \
  --file "${work_dir}/${MDBOOK_ARCHIVE}" \
  --directory "${work_dir}"

"${work_dir}/mdbook" build
