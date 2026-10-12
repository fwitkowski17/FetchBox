#!/usr/bin/env bash
# Full build: frontend first (embedded into the binary), then the release binary.
set -euo pipefail
cd "$(dirname "$0")/.."

(cd web && npm ci && npm run build)
cargo build --release -p server

echo "Built: target/release/dlmanager"
