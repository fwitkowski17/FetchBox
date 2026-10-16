#!/usr/bin/env bash
# Puts a new version everywhere it has to be the same: the Cargo workspace (and its lock file),
# the web app (and its lock file) and the generated OpenAPI document, which carries it.
#
#   scripts/bump-version.sh 0.7.0        # or 0.7.0-rc.1 for a pre-release
#
# Commit the result and merge it into main: .github/workflows/tag.yml then tags it and starts the
# release (see CONTRIBUTING.md, "Releasing").
set -euo pipefail

version="${1:-}"
if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$ ]]; then
  echo "usage: $0 <version>   e.g. 0.7.0 or 0.7.0-rc.1" >&2
  exit 2
fi

cd "$(dirname "$0")/.."

# [workspace.package] version = "..."
current="$(sed -n '/^\[workspace\.package\]/,/^\[/{s/^version = "\(.*\)"/\1/p}' Cargo.toml | head -n1)"
[[ -n "$current" ]] || {
  echo "error: no version found in Cargo.toml" >&2
  exit 1
}
if [[ "$current" == "$version" ]]; then
  echo "already at ${version}"
  exit 0
fi

sed -i '/^\[workspace\.package\]/,/^\[/{s/^version = ".*"/version = "'"$version"'"/}' Cargo.toml
(cd web && npm version "$version" --no-git-tag-version --allow-same-version >/dev/null)
cargo update --workspace --quiet
./scripts/gen-api.sh >/dev/null

echo "${current} -> ${version}"
git status --short
