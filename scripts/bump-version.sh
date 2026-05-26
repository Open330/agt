#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:?Usage: $0 <version>}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Portable in-place sed (BSD/GNU): write to temp, then replace atomically.
# Single -e expression per call to avoid BSD multi-statement quirks.
inplace() {
  local expr="$1" file="$2"
  local tmp
  tmp="$(mktemp)"
  sed -e "$expr" "$file" > "$tmp"
  mv "$tmp" "$file"
}

# agt/Cargo.toml — the only "^version = " line is in [package]
inplace 's/^version = "[^"]*"/version = "'"$VERSION"'"/' "$ROOT/agt/Cargo.toml"
echo "Updated agt/Cargo.toml -> $VERSION"

# npm/package.json: top-level "version" + every "@open330/agt-*" optionalDependency
inplace 's/"version": "[^"]*"/"version": "'"$VERSION"'"/' "$ROOT/npm/package.json"
inplace 's|"@open330/agt-\([^"]*\)": "[^"]*"|"@open330/agt-\1": "'"$VERSION"'"|g' "$ROOT/npm/package.json"
echo "Updated npm/package.json -> $VERSION"

# Platform package.json files (only active platforms tracked in optionalDependencies)
for pkg in darwin-arm64 linux-x64 linux-arm64; do
  manifest="$ROOT/npm/platforms/$pkg/package.json"
  [ -f "$manifest" ] || continue
  inplace 's/"version": "[^"]*"/"version": "'"$VERSION"'"/' "$manifest"
  echo "Updated npm/platforms/$pkg/package.json -> $VERSION"
done

echo "Done. All versions set to $VERSION"
