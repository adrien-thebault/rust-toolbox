#!/usr/bin/env bash
# Regenerate the gateway's committed OpenAPI spec.
#
# CI runs the same command and fails on a diff, so a route whose schema changed
# without this being run is a build failure naming exactly what moved. Commit
# web/openapi.json the way you commit Cargo.lock.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")"
cargo run --quiet -p {{project-name}}-web --example dump-openapi > web/openapi.json
echo "wrote web/openapi.json"
