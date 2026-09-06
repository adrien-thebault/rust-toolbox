#!/usr/bin/env bash
# Workspace line coverage. Builds instrumented, runs the tests, writes an HTML
# report under target/llvm-cov/html/ and opens it. Extra arguments are passed
# through to `cargo llvm-cov`.
#
#   ./scripts/coverage.sh
#
# Needs: cargo install cargo-llvm-cov  &&  rustup component add llvm-tools-preview
#
# The examples are excluded: they exist to exercise the macros, not to be
# measured. toolbox-db's postgres/mysql paths show uncovered unless the
# TOOLBOX_TEST_*_URL variables point at running servers; doctests need nightly.
set -euo pipefail

if ! command -v cargo-llvm-cov >/dev/null 2>&1; then
    echo "cargo-llvm-cov is not installed. Install it with: cargo install cargo-llvm-cov" >&2
    exit 1
fi

cd "$(dirname "${BASH_SOURCE[0]}")/.."
exec cargo llvm-cov --workspace --all-features \
    --exclude example-todo --exclude example-web \
    --open "$@"
