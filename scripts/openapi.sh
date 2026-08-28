#!/usr/bin/env bash
# Regenerate the example's committed OpenAPI spec.
#
# CI runs the same command and fails on a diff, so a route whose schema changed
# without this being run is a build failure naming exactly what moved.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
cargo run --quiet -p example-web --example dump-openapi > examples/grpc-cluster/web/openapi.json
echo "wrote examples/grpc-cluster/web/openapi.json"
