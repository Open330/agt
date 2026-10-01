#!/usr/bin/env bash
# Validate that the checked-in release version is consistent everywhere.
#
# Source of truth: agt/Cargo.toml [package].version
# Checked against: agt/Cargo.lock, npm/package.json, npm/platforms/*/package.json
#                  and npm/package.json optionalDependencies.
#
# Usage:
#   scripts/check-versions.sh            # validate consistency, print version
#   scripts/check-versions.sh 2026.7.23  # also require this exact version
set -euo pipefail

expected="${1-}"
repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$repo_root"

cargo_version="$(awk '
  /^\[package\]$/ { in_package = 1; next }
  in_package && /^\[/ { exit }
  in_package && /^version = "/ {
    value = $0
    sub(/^version = "/, "", value)
    sub(/"$/, "", value)
    print value
    exit
  }
' agt/Cargo.toml)"

lock_version="$(awk '
  /^\[\[package\]\]$/ { in_agt = 0; next }
  /^name = "agt"$/ { in_agt = 1; next }
  in_agt && /^version = "/ {
    value = $0
    sub(/^version = "/, "", value)
    sub(/"$/, "", value)
    print value
    exit
  }
' agt/Cargo.lock)"

if [[ ! "$cargo_version" =~ ^[0-9]{4}\.(0?[1-9]|1[0-2])\.(0?[1-9]|[12][0-9]|3[01])$ ]]; then
  echo "Invalid Cargo release version: expected YYYY.M.D, got '$cargo_version'" >&2
  exit 1
fi

if [ "$lock_version" != "$cargo_version" ]; then
  echo "Version mismatch: agt/Cargo.lock=$lock_version, agt/Cargo.toml=$cargo_version" >&2
  exit 1
fi

for manifest in npm/package.json npm/platforms/*/package.json; do
  manifest_version="$(node -p "require('./$manifest').version")"
  if [ "$manifest_version" != "$cargo_version" ]; then
    echo "Version mismatch: $manifest=$manifest_version, agt/Cargo.toml=$cargo_version" >&2
    exit 1
  fi
done

for package_name in \
  @open330/agt-darwin-arm64 \
  @open330/agt-linux-x64 \
  @open330/agt-linux-arm64; do
  dependency_version="$(node -p "require('./npm/package.json').optionalDependencies['$package_name']")"
  if [ "$dependency_version" != "$cargo_version" ]; then
    echo "Version mismatch: npm optionalDependency $package_name=$dependency_version, agt/Cargo.toml=$cargo_version" >&2
    exit 1
  fi
done

if [ -n "$expected" ] && [ "$expected" != "$cargo_version" ]; then
  echo "Expected version '$expected' does not match checked-in version '$cargo_version'" >&2
  exit 1
fi

echo "$cargo_version"
