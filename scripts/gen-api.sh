#!/usr/bin/env bash
# Regenerate the OpenAPI document and the frontend types derived from it.
# Run after changing anything in the HTTP API, then commit docs/openapi.json and
# web/src/lib/api/schema.d.ts (never edit the generated files by hand).
set -euo pipefail
cd "$(dirname "$0")/.."

# Types are generated with a pinned openapi-typescript through npx: v7 still declares a
# peer dependency on TypeScript 5 while the web project uses 6, so it is deliberately not
# a devDependency. Needs network access the first time.
cargo run -q -p server -- --print-openapi > docs/openapi.json
(cd web && npm run --silent gen:api)

echo "Updated docs/openapi.json and web/src/lib/api/schema.d.ts"
