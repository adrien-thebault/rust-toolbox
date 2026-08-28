#!/usr/bin/env bash
# Run the gRPC cluster example end to end: a real backend on a real socket, an
# in-process gateway, and a browser-shaped request going all the way through.
#
#   ./scripts/example.sh              # the whole suite
#   ./scripts/example.sh admin        # only tests whose name contains "admin"
#
# Needs protoc. `docker compose run --rm test` in examples/grpc-cluster needs
# nothing but Docker.
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
exec cargo test -p example-todo -p example-web -- "$@"
