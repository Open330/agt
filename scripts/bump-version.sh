#!/usr/bin/env bash
set -euo pipefail

VERSION="${1:?Usage: $0 <version>}"
ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# agt/Cargo.toml
sed -i '' "s/^version = \".*\"/version = \"$VERSION\"/" "$ROOT/agt/Cargo.toml"
echo "Updated agt/Cargo.toml -> $VERSION"

# npm/package.json: version + optionalDependencies
sed -i '' "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" "$ROOT/npm/package.json"
sed -i '' "s/\"@open330\/agt-\([^\"]*\)\": \"[^\"]*\"/\"@open330\/agt-\1\": \"$VERSION\"/g" "$ROOT/npm/package.json"
echo "Updated npm/package.json -> $VERSION"

# Platform package.json files
for pkg in darwin-arm64 linux-x64; do
  sed -i '' "s/\"version\": \".*\"/\"version\": \"$VERSION\"/" "$ROOT/npm/platforms/$pkg/package.json"
  echo "Updated npm/platforms/$pkg/package.json -> $VERSION"
done

echo "Done. All versions set to $VERSION"
