#!/usr/bin/env bash
# Print an argon2 hash for a password, in the PHC format a UserStore expects.
#
#   ./scripts/hash-password.sh 'correct horse battery staple'
#   ./scripts/hash-password.sh      # prompts, so it stays out of your history
set -euo pipefail
cd "$(dirname "${BASH_SOURCE[0]}")/.."
exec cargo run --quiet --package toolbox-auth --features password --example hash-password -- "$@"
