#!/usr/bin/env bash
# Runs the suite once per meaningful feature combination (mirrored by
# .github/workflows/ci.yml). sqlite/postgresql/mysql are mutually exclusive
# diesel backends, so each gets its own run rather than being combined with
# the others or with --all-features (which fails to compile for exactly that
# reason).
#
# The postgresql/mysql integration tests need a real server: point
# TOOLBOX_TEST_POSTGRES_URL / TOOLBOX_TEST_MYSQL_URL at one (CI uses service
# containers). Without the variable the DB tests soft-skip, so those runs
# still verify compilation and the backend-independent tests.
set -euo pipefail

cd "$(dirname "${BASH_SOURCE[0]}")/.."

feature_sets=(
    "axum,mail,tower"
    "sqlite"
    "postgresql"
    "mysql"
)

for features in "${feature_sets[@]}"; do
    echo "==> cargo test --no-default-features --features $features"
    cargo test --no-default-features --features "$features"
done
